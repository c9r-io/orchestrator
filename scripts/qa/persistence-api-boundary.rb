#!/usr/bin/env ruby

# Freezes who can obtain a SQLite connection, against
# config/governance/persistence-api-boundary-ledger.json.
#
# DD-147 froze two things: who may *declare* the driver (manifests, by section)
# and who may *use* it (per-file SQL statements and driver references). Neither
# observes the fact underneath both: `AsyncDatabase::writer()` returns
# `&tokio_rusqlite::Connection`, and `db::open_conn(path)` returns an owned
# `rusqlite::Connection` without going through `AsyncDatabase` at all. A crate
# handed a connection runs `conn.execute(sql, [])` with no `rusqlite::` token
# anywhere, so condition 1 reports it clean; condition 2 counts what it did with
# the connection, not that it could get one.
#
# FR-141 measured the gap: 54 `writer()`/`reader()` call sites and 27
# `open_conn(path)` call sites in production source outside the layer, across
# three crates that are forbidden to hold the driver.
#
# Three facts, reported independently, because no two of them substitute:
#
#   1. YIELDS — a public item whose return position names a driver connection.
#      This is the capability being handed out.
#
#   2. DEMANDS — a public item whose parameter position names a driver type.
#      A function that demands a connection forces its callers to acquire one,
#      which is why `orchestrator-security` — exempt, below core, opening its
#      own connection — is the reason `crates/daemon` opens connections at all.
#      An exempt crate's API shape can push the driver upward past the layer.
#
#   3. HOLDS — production source outside the layer that calls something this
#      gate itself classified as YIELDS. The scanned names are derived from
#      fact 1 rather than listed here: `writer`, `reader` and `open_conn` are
#      what the tree happens to contain today, and a hand-written list of them
#      would guard exactly today's tree. When a name stops yielding a
#      connection it stops being scanned for, and a new one is scanned for the
#      moment it appears.
#
# Two things this gate deliberately does not do, because the rule is already
# stated once elsewhere. It does not check whether a crate declares the driver —
# that is DD-147 condition 1, in persistence-dependency.rb. And it does not
# count SQL statements — that is condition 2, in the same place. A crate calling
# `rusqlite::Connection::open` directly, bypassing every name here, must first
# declare the driver, and condition 1 fails on that.
#
# Usage:
#   persistence-api-boundary.rb                  verify the repository
#   persistence-api-boundary.rb --emit-baseline  print the candidate ledger
#   persistence-api-boundary.rb --emit-baseline --write   apply it locally

require "json"
require "optparse"
require "pathname"
require_relative "../lib/rust_source"
require_relative "../lib/ci_env"

# The same scanner the other three ledgers use. Counting one tree four ways
# produces four reviewed states that all look correct.
include RustSource

SCOPE = "every workspace member listed in the root Cargo.toml and its non-test " \
  "Rust source — its src tree and its Cargo build script — excluding inline " \
  "cfg(test) modules, files under a tests directory, and files named test*.rs; " \
  "public API is resolved through the module tree and its re-exports, and driver " \
  "types through each file's own use statements".freeze

DRIVER_CRATES = %w[rusqlite tokio_rusqlite].freeze
# A path-qualified driver type, whatever the file imported.
DRIVER_PATH = /\b(?:#{DRIVER_CRATES.join("|")})\s*::\s*(\w+)/
# The capability itself, as opposed to the driver's other exported types. Only
# these three let the holder run arbitrary SQL; `Error`, `OpenFlags` and the
# conversion traits leak the crate without leaking the database.
CONNECTION_TYPES = %w[Connection Transaction Savepoint].freeze

options = { ledger: "config/governance/persistence-api-boundary-ledger.json", emit_baseline: false, write: false }
OptionParser.new do |parser|
  parser.on("--ledger PATH") { |value| options[:ledger] = value }
  parser.on("--emit-baseline") { options[:emit_baseline] = true }
  parser.on("--write") { options[:write] = true }
end.parse!

repo_root = Pathname.new(File.expand_path("../..", __dir__))
ledger_path = repo_root.join(options[:ledger])

# ---------------------------------------------------------------------------
# Workspace discovery. Shared shape with persistence-dependency.rb: the
# authority on what a member is, is the [workspace] table, not a crates/* glob.
# ---------------------------------------------------------------------------

def workspace_members(repo_root)
  manifest = File.read(repo_root.join("Cargo.toml"))
  section = manifest[/^\[workspace\]\s*$(.*?)(?=^\[|\z)/m]
  return [] unless section

  list = section[/^\s*members\s*=\s*\[(.*?)\]/m, 1]
  return [] unless list

  list.scan(/"([^"]+)"/).flatten.flat_map do |entry|
    if entry.include?("*")
      Dir[repo_root.join(entry, "Cargo.toml").to_s]
        .map { |path| relative_path(repo_root, Pathname.new(path).dirname) }
    else
      [entry]
    end
  end.uniq.sort
end

def member_roots(repo_root, members)
  members.flat_map do |member|
    [repo_root.join(member, "src"), repo_root.join(member, "build.rs")]
  end.select(&:exist?)
end

# ---------------------------------------------------------------------------
# Source access. Lexical masking is the whole cost of this scan — 13 seconds
# across the workspace — so each file is read once, masked at most once, and
# only when something cheap says it could matter. Every prefilter below tests
# the RAW text for a substring the masked text can only have less of, so a
# prefilter can skip work but cannot hide a finding.
# ---------------------------------------------------------------------------

SOURCE_CACHE = {}

def raw_source(path)
  entry = (SOURCE_CACHE[path.to_s] ||= {})
  entry[:raw] ||= File.read(path)
end

# The stripped, masked copy: comments, strings and cfg(test) modules all gone.
# strip_test_modules is handed the masked source as both arguments so the mask
# is computed once rather than once here and once inside it.
def masked_source(path)
  entry = (SOURCE_CACHE[path.to_s] ||= {})
  return entry[:masked] if entry.key?(:masked)

  masked = RustLexer.mask_literals(raw_source(path))
  entry[:masked] = RustSource.strip_test_modules(masked, masked)
end

# A file with no `mod x;` declaration and no `pub use` has no children and
# re-exports nothing, so the module walk learns nothing by masking it.
RAW_MOD_DECL = /^[ \t]*(?:pub[^\n]*?[ \t])?mod[ \t]+\w+[ \t]*;/
RAW_PUB_USE = /^[ \t]*pub[ \t]+use[ \t]/

def could_declare_modules?(path)
  raw = raw_source(path)
  raw.match?(RAW_MOD_DECL) || raw.match?(RAW_PUB_USE)
end

# Every driver type this gate can find is either written with a `rusqlite::` or
# `tokio_rusqlite::` path, or imported by a `use` statement that contains one.
# A file without the substring cannot produce a finding.
def could_name_driver?(path)
  raw_source(path).include?("rusqlite")
end

# ---------------------------------------------------------------------------
# Module tree. `pub fn` in a file is not public API: crates/orchestrator-
# persistence/src/task_repository/mod.rs declares `mod items;` and `mod
# write_ops;` privately and re-exports four names out of the seventeen public
# functions those two files define. A file-level heuristic reports all
# seventeen, and the thirteen it invents are the ones a migration would then be
# told to move for no reason.
# ---------------------------------------------------------------------------

MOD_DECL = /^[ \t]*(pub(?:\s*\([^)]*\))?[ \t]+)?mod[ \t]+([A-Za-z_]\w*)[ \t]*;/
PUB_USE = /^[ \t]*pub[ \t]+use[ \t]+([^;]+);/m

# Resolves `mod name;` to a file, Rust 2018 style: a sibling `name.rs`, or
# `name/mod.rs` under the declaring module's directory.
def child_module_file(declaring_file, name)
  dir = if declaring_file.basename.to_s == "lib.rs" || declaring_file.basename.to_s == "mod.rs"
          declaring_file.dirname
        else
          declaring_file.dirname.join(declaring_file.basename(".rs").to_s)
        end
  flat = dir.join("#{name}.rs")
  return flat if flat.file?

  nested = dir.join(name, "mod.rs")
  return nested if nested.file?

  nil
end

# The names a file re-exports out of each of its child modules. `pub use x::*`
# records :glob, which republishes everything the child makes public.
def reexports(masked)
  found = Hash.new { |hash, key| hash[key] = [] }
  masked.scan(PUB_USE) do |(body)|
    path = body.strip
    # Only re-exports from a child module of this file matter for visibility;
    # `pub use crate::…` and `pub use some_crate::…` name items that are already
    # public where they are defined.
    next if path.start_with?("crate::", "super::", "::")

    head = path[/\A([A-Za-z_]\w*)\s*::/, 1]
    next unless head

    rest = path[(path.index("::") + 2)..].to_s.strip
    if rest == "*"
      found[head] = :glob
    elsif rest.start_with?("{")
      next if found[head] == :glob

      rest[/\{(.*)\}/m, 1].to_s.split(",").each do |entry|
        entry = entry.strip
        next if entry.empty?
        # `insert_event as insert_event_row` publishes the item named on the
        # left; the alias is what callers write, not what is defined.
        found[head] << entry.split(/\s+as\s+/).first.to_s.strip.split("::").last
      end
    else
      next if found[head] == :glob

      found[head] << rest.split(/\s+as\s+/).first.to_s.strip.split("::").last
    end
  end
  found
end

# Walks from a crate root and returns { absolute path => exported-name filter },
# where the filter is :all for a publicly reachable module, or an array of the
# names a private module has re-exported out of it.
# A `#[cfg(feature = "…")]` sitting immediately before a `mod` declaration.
# Read against the raw source rather than the masked copy: masking blanks the
# inside of the string, and the feature's name is what decides the classification.
# `mask_literals` replaces characters in place, so an offset means the same thing
# in both.
CFG_FEATURE = /\#\[\s*cfg\s*\(\s*feature\s*=\s*"([^"]+)"\s*\)\s*\]\s*\z/m

# Returns [reachable, test_only] — the public module files of a crate, and the
# subset of them reached only through a module gated on a test-only feature.
#
# The gated subtree is reported, not skipped. A gate that dropped it would
# certify an exemption it cannot observe, which is the failure §4.4 names: the
# module's findings are counted into their own bucket, and the condition that
# makes the bucket harmless — that no crate enables the feature from
# `[dependencies]` — is asserted separately in `feature_edge_errors`.
def public_module_files(crate_root_file, test_only_features = [])
  return [{}, {}] unless crate_root_file.file?

  reachable = {}
  test_only = {}
  queue = [[crate_root_file, :all, false]]
  seen = {}

  until queue.empty?
    file, filter, gated = queue.shift
    key = file.to_s
    # A module reached both publicly and by re-export keeps the wider filter.
    if seen[key]
      next unless filter == :all && seen[key] != :all

      seen[key] = :all
    else
      seen[key] = filter
    end
    reachable[file] = seen[key]
    test_only[file] = true if gated
    test_only.delete(file) unless gated || test_only[file].nil?
    next unless could_declare_modules?(file)

    masked = masked_source(file)
    raw = raw_source(file)
    exported = reexports(masked)

    masked.scan(MOD_DECL) do |(visibility, name)|
      offset = Regexp.last_match.begin(0)
      child = child_module_file(file, name)
      next unless child

      feature = raw[0...offset][CFG_FEATURE, 1]
      child_gated = gated || test_only_features.include?(feature)

      public_mod = !visibility.nil? && visibility.strip == "pub"
      child_filter =
        if public_mod && filter == :all
          :all
        elsif exported[name] == :glob
          :all
        elsif exported.key?(name) && !exported[name].empty?
          exported[name]
        end
      queue << [child, child_filter, child_gated] if child_filter
    end
  end

  [reachable, test_only]
end

# Which dependency table each crate uses to enable a test-only feature.
#
# This is the condition that makes the gated door safe, and it is asserted
# rather than assumed: under resolver 2 a feature enabled from
# `[dev-dependencies]` is not unified into a normal build, so the module does
# not exist in the shipped artifact. Enabled from `[dependencies]`, it does,
# and the hole is a real one. Parsed per table rather than by searching the
# file for the feature's name, because the name appears in both tables and
# only the table it appears in decides.
def feature_edge_errors(repo_root, members, test_only_features)
  return [] if test_only_features.empty?

  errors = []
  members.each do |member|
    manifest = repo_root.join(member, "Cargo.toml")
    next unless manifest.file?

    table = nil
    File.readlines(manifest).each_with_index do |line, index|
      stripped = line.strip
      if stripped.start_with?("[")
        table = stripped
        next
      end
      next unless table
      # A dev-dependency table is the sanctioned edge; a target-specific or
      # build table is not, and neither is a plain [dependencies].
      next if table.include?("dev-dependencies")
      next unless table.include?("dependencies")

      feature = test_only_features.find do |name|
        line.include?("features") && line.include?("\"#{name}\"")
      end
      next unless feature

      errors << "  #{member}/Cargo.toml:#{index + 1} enables the test-only feature " \
        "#{feature.inspect} from #{table}, not [dev-dependencies]; the module it " \
        "gates would then exist in a production build"
    end
  end
  errors
end

# ---------------------------------------------------------------------------
# Item parsing. Signatures are taken by bracket matching over lexically masked
# source, not by line. `->` puts a `>` in every returning signature, so angle
# brackets are deliberately not counted as depth; a signature ends at the first
# `{` or `;` outside parentheses and square brackets.
# ---------------------------------------------------------------------------

ITEM = /(?:^|\n)[ \t]*(pub(?:\s*\([^)]*\))?[ \t]+)?(?:async[ \t]+|const[ \t]+|unsafe[ \t]+|extern[ \t]+"[^"]*"[ \t]+)*(fn|struct|enum|union|trait|type|const|static)[ \t]+([A-Za-z_]\w*)/

def signature_after(masked, offset)
  depth = 0
  text = +""
  index = offset
  while index < masked.length
    char = masked[index]
    break if depth.zero? && (char == "{" || char == ";")

    depth += 1 if char == "(" || char == "["
    depth -= 1 if char == ")" || char == "]"
    text << char
    index += 1
  end
  [text, index]
end

def balanced_body(masked, brace_index)
  return "" unless masked[brace_index] == "{"

  depth = 0
  index = brace_index
  while index < masked.length
    depth += 1 if masked[index] == "{"
    depth -= 1 if masked[index] == "}"
    return masked[(brace_index + 1)...index] if depth.zero? && index > brace_index

    index += 1
  end
  masked[(brace_index + 1)..].to_s
end

# Splits a `fn` signature into its parameter list and everything else (return
# type, generic bounds, where clause). Generics between the name and the
# parameters are skipped by angle-bracket depth, which is only safe here because
# it is bounded to that one span.
def split_fn_signature(signature)
  cursor = 0
  if (open_angle = signature.index("<"))
    paren = signature.index("(")
    if paren.nil? || open_angle < paren
      depth = 0
      index = open_angle
      while index < signature.length
        depth += 1 if signature[index] == "<"
        depth -= 1 if signature[index] == ">"
        if depth.zero?
          cursor = index + 1
          break
        end
        index += 1
      end
    end
  end

  open_paren = signature.index("(", cursor)
  return ["", signature] unless open_paren

  depth = 0
  index = open_paren
  while index < signature.length
    depth += 1 if signature[index] == "("
    depth -= 1 if signature[index] == ")"
    break if depth.zero? && index > open_paren

    index += 1
  end
  params = signature[(open_paren + 1)...index].to_s
  rest = signature[(index + 1)..].to_s
  [params, rest]
end

# The driver type names this file can write without a path: everything it
# imported from rusqlite or tokio_rusqlite, under whatever local name. A gate
# that matched the literal word `Connection` is defeated by
# `use rusqlite::Connection as Db;`, which is why the alias is read rather than
# the token.
def driver_aliases(masked)
  aliases = {}
  masked.scan(/^[ \t]*(?:pub[ \t]+)?use[ \t]+((?:#{DRIVER_CRATES.join("|")})::[^;]+);/) do |(path)|
    body = path.split("::", 2).last.to_s.strip
    entries =
      if body.start_with?("{")
        body[/\{(.*)\}/m, 1].to_s.split(",")
      else
        [body]
      end
    entries.each do |entry|
      entry = entry.strip
      next if entry.empty? || entry == "*"

      source_name, local = entry.split(/\s+as\s+/).map { |part| part.to_s.strip }
      source_name = source_name.split("::").last
      local = (local || source_name).split("::").last
      next if local.nil? || local.empty?

      aliases[local] = source_name
    end
  end
  aliases
end

# Which driver types a type expression names, as their canonical driver names.
def driver_types_in(expression, aliases)
  found = []
  expression.scan(DRIVER_PATH) { |(name)| found << name }
  aliases.each do |local, canonical|
    found << canonical if expression.match?(/(?<![A-Za-z0-9_:])#{Regexp.escape(local)}(?![A-Za-z0-9_])/)
  end
  found.uniq
end

def connection_types(names)
  names & CONNECTION_TYPES
end

# Public fields of a struct or enum body, and the method signatures of a trait
# body. A public field of driver type hands the connection out as surely as a
# getter does.
#
# Bare `pub` only. The item-level test above already reads `pub(crate) fn` as
# not crate-external, and a field is no different: `pub(crate) up` is reachable
# from inside the crate and nowhere else. Matching `pub(…)` here reported
# `struct Migration` as still yielding a connection after FR-141 B5a made its
# `up` field crate-private — a false positive, which inflates the count without
# ever hiding a leak, and inflating it is enough to make a closed door look open.
def nested_public_signatures(kind, body)
  case kind
  when "struct", "union"
    body.scan(/(?:^|,)\s*pub\s+[A-Za-z_]\w*\s*:([^,]*)/).flatten +
      body.scan(/(?:^|\()\s*pub\s+([^,)]*)/).flatten
  when "trait"
    body.scan(/(?:^|\n)\s*(?:async\s+|unsafe\s+)*fn[ \t]+[A-Za-z_]\w*([^;{]*)/).flatten
  else
    []
  end
end

Finding = Struct.new(:file, :item, :position, :types, keyword_init: true)

# Every crate-external item of one file, split by whether the driver type sits
# where the caller supplies it or where the callee returns it.
def public_api_findings(repo_root, file, filter)
  return [] unless could_name_driver?(file)

  masked = masked_source(file)
  aliases = driver_aliases(masked)
  relative = relative_path(repo_root, file)
  findings = []

  masked.to_enum(:scan, ITEM).each do
    match = Regexp.last_match
    visibility = match[1]
    kind = match[2]
    name = match[3]
    next unless visibility && visibility.strip == "pub"
    next unless filter == :all || Array(filter).include?(name)

    signature, stop = signature_after(masked, match.end(0))
    body = masked[stop] == "{" ? balanced_body(masked, stop) : ""

    positions = {}
    if kind == "fn"
      params, rest = split_fn_signature(signature)
      positions["parameter"] = params
      positions["return"] = rest
    elsif kind == "type"
      positions["return"] = signature.split("=", 2).last.to_s
    elsif %w[const static].include?(kind)
      positions["return"] = signature.split(":", 2).last.to_s
    else
      positions["return"] = signature
    end

    nested_public_signatures(kind, body).each_with_index do |nested, index|
      if kind == "trait"
        params, rest = split_fn_signature("(#{nested.split("(", 2).last}")
        positions["parameter"] = [positions["parameter"], params].compact.join(" ")
        positions["return"] = [positions["return"], rest].compact.join(" ")
      else
        positions["return"] = [positions["return"], nested].compact.join(" ")
      end
      index
    end

    positions.each do |position, expression|
      next if expression.nil? || expression.strip.empty?

      types = driver_types_in(expression, aliases)
      next if types.empty?

      findings << Finding.new(file: relative, item: "#{kind} #{name}", position: position, types: types.sort)
    end
  end

  findings
end

# ---------------------------------------------------------------------------
# Snapshot
# ---------------------------------------------------------------------------

def reviewed_half(ledger_path)
  return { "decision" => {}, "roles" => {} } unless ledger_path.file?

  ledger = JSON.parse(File.read(ledger_path))
  {
    "decision" => ledger["decision"] || {},
    "roles" => ledger["roles"] || {},
    "testOnlyFeatures" => ledger["testOnlyFeatures"] || []
  }
end

def snapshot(repo_root, ledger_path)
  reviewed = reviewed_half(ledger_path)
  roles = reviewed["roles"]
  test_only_features = reviewed["testOnlyFeatures"]
  members = workspace_members(repo_root)

  yields = {}
  demands = {}
  test_only_yields = {}
  test_only_demands = {}
  yielding_names = []

  members.each do |member|
    role = (roles[member] || {})["role"]
    next if role == "separate-database"

    crate_root = repo_root.join(member, "src", "lib.rs")
    crate_root = repo_root.join(member, "src", "main.rs") unless crate_root.file?
    reachable, gated = public_module_files(crate_root, test_only_features)
    reachable.each do |file, filter|
      public_api_findings(repo_root, file, filter).each do |finding|
        key = "#{finding.file}::#{finding.item}"
        entry = { "crate" => member, "types" => finding.types }
        if finding.position == "return"
          (gated[file] ? test_only_yields : yields)[key] = entry
          # Fact 3 scans for the gated names too. The module is unreachable from
          # a production build, so a production file naming one is either a
          # compile error waiting to happen or the feature leaking through a
          # `[dependencies]` edge — both worth reporting, and neither observable
          # if the name were dropped from the search here.
          yielding_names << finding.item.split(" ").last unless connection_types(finding.types).empty?
        else
          (gated[file] ? test_only_demands : demands)[key] = entry
        end
      end
    end
  end

  # Fact 3's scanned names are fact 1's output, not a list. A name that stops
  # yielding a connection stops being looked for on the same run.
  yielding_names = yielding_names.uniq.sort
  call_site = yielding_names.empty? ? nil : /(?<![A-Za-z0-9_])(#{yielding_names.map { |n| Regexp.escape(n) }.join("|")})\s*\(/

  holds = {}
  unless call_site.nil?
    layer_members = members.select do |member|
      role = (roles[member] || {})["role"]
      %w[persistence exempt separate-database].include?(role)
    end
    scanned = members - layer_members
    rust_files_under(repo_root, member_roots(repo_root, scanned)).each do |path|
      # Raw prefilter: masking can only remove occurrences of a name, so a file
      # whose raw text contains none of them cannot contain one after masking.
      next unless yielding_names.any? { |name| raw_source(path).include?(name) }

      hits = masked_source(path).scan(call_site).flatten
      next if hits.empty?

      relative = relative_path(repo_root, path)
      member = scanned.select { |root| relative.start_with?("#{root}/") }.max_by(&:length)
      holds[relative] = {
        "crate" => member,
        "acquisitions" => hits.length,
        "via" => hits.uniq.sort
      }
    end
  end

  {
    "schemaVersion" => 1,
    "scope" => SCOPE,
    "decision" => reviewed["decision"],
    "testOnlyFeatures" => test_only_features,
    "roles" => roles,
    "yields" => yields.sort.to_h,
    "demands" => demands.sort.to_h,
    "testOnlyYields" => test_only_yields.sort.to_h,
    "testOnlyDemands" => test_only_demands.sort.to_h,
    "holds" => holds.sort.to_h,
    "totals" => {
      "yields" => yields.length,
      "demands" => demands.length,
      "testOnlyYields" => test_only_yields.length,
      "testOnlyDemands" => test_only_demands.length,
      "holdingFiles" => holds.length,
      "acquisitions" => holds.values.sum { |entry| entry["acquisitions"] }
    }
  }
end

def map_report(label, expected, actual, describe)
  before = expected || {}
  after = actual || {}
  lines = []
  (after.keys - before.keys).sort.each { |key| lines << "  + #{key} #{describe.call(after[key])} and is not in the ledger" }
  (before.keys - after.keys).sort.each { |key| lines << "  - #{key} is gone; the ledger still records it" }
  (before.keys & after.keys).sort.each do |key|
    next if before[key] == after[key]

    lines << "  ~ #{key} #{describe.call(before[key])} -> #{describe.call(after[key])}"
  end
  lines.empty? ? [] : ["#{label} differ from the reviewed ledger:\n#{lines.join("\n")}"]
end

actual = snapshot(repo_root, ledger_path)

if options[:emit_baseline]
  if options[:write]
    CiEnv.refuse_unattended_write!(
      "ledger",
      "run --emit-baseline locally, read the diff, and commit the ledger with the change"
    )
    File.write(ledger_path, ledger_json(actual))
    warn "wrote #{options[:ledger]}; review the diff and commit it with the change that caused it"
    exit 0
  end
  print ledger_json(actual)
  exit 0
end

if options[:write]
  warn "--write requires --emit-baseline"
  exit 2
end

unless ledger_path.file?
  warn "persistence API boundary ledger not found at #{options[:ledger]}"
  warn "generate it with --emit-baseline --write and commit it"
  exit 1
end

expected = JSON.parse(File.read(ledger_path))
errors = []

if expected["scope"] != SCOPE
  errors << "ledger scope prose does not match the scan this gate implements; " \
    "the ledger describes something the gate does not measure"
end

# Every member the decision map names must still be a workspace member, or the
# decision is being applied to nothing.
members = workspace_members(repo_root)
(expected["roles"] || {}).each_key do |member|
  next if members.include?(member)

  errors << "roles names #{member}, which is not a workspace member"
end

errors.concat(map_report("public items yielding a driver type", expected["yields"], actual["yields"],
                         ->(entry) { "returns #{Array(entry["types"]).join("/")}" }))
errors.concat(map_report("public items demanding a driver type", expected["demands"], actual["demands"],
                         ->(entry) { "takes #{Array(entry["types"]).join("/")}" }))
errors.concat(map_report("files acquiring a connection outside the layer", expected["holds"], actual["holds"],
                         ->(entry) { "#{entry["acquisitions"]} acquisition(s) via #{Array(entry["via"]).join("/")}" }))
errors.concat(map_report("test-only public items yielding a driver type", expected["testOnlyYields"],
                         actual["testOnlyYields"],
                         ->(entry) { "returns #{Array(entry["types"]).join("/")}" }))
errors.concat(map_report("test-only public items demanding a driver type", expected["testOnlyDemands"],
                         actual["testOnlyDemands"],
                         ->(entry) { "takes #{Array(entry["types"]).join("/")}" }))

# `testOnlyFeatures` is a reviewed field like `decision` and `roles`: the gate
# reads it rather than deriving it, so comparing it against itself would assert
# nothing. What is asserted is the consequence — everything the listed features
# gate is inventoried above, and the edge condition below.
#
# Not a ledger diff. A test-only feature reachable from [dependencies] is a
# production hole however faithfully the ledger records what it exposes, so
# this fails on the fact itself rather than on a disagreement about it.
errors.concat(feature_edge_errors(repo_root, members, expected["testOnlyFeatures"] || []))

if errors.empty?
  totals = actual["totals"]
  puts "Persistence API boundary: PASS"
  puts "  public API: #{totals["yields"]} item(s) yield a driver type, " \
    "#{totals["demands"]} demand one"
  puts "  test-only door: #{totals["testOnlyYields"]} item(s) yield a driver type, " \
    "#{totals["testOnlyDemands"]} demand one, behind " \
    "#{(actual["testOnlyFeatures"] || []).join("/")} and reachable from [dev-dependencies] only"
  puts "  outside the layer: #{totals["acquisitions"]} connection acquisition(s) " \
    "across #{totals["holdingFiles"]} file(s)"
  exit 0
end

warn "Persistence API boundary: FAIL"
errors.each { |error| warn "  #{error}" }
warn "  regenerate with --emit-baseline, review the diff, and commit the ledger " \
  "together with the change that caused it"
exit 1
