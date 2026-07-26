#!/usr/bin/env ruby

# Holds the FR-136 persistence chokepoint decision against
# config/governance/persistence-dependency-ledger.json.
#
# FR-130 froze core's boundary and found that core is not the persistence
# chokepoint: six crates take the SQLite driver directly. Extracting
# orchestrator-persistence without first deciding who may depend on it produces
# a new crate that five crates depend on instead of core — the driver's blast
# radius unchanged, a god crate traded for a god dependency. This gate is that
# decision, made executable.
#
# Two independent conditions, because either one alone certifies an enforcement
# it cannot observe:
#
#   1. Who may DECLARE the driver. Read from the member manifests, discovered
#      from the workspace `members` list and parsed by section so
#      [dependencies] and [dev-dependencies] are different facts.
#
#   2. Who may USE it. A manifest says nothing about volume, and nothing at all
#      about a crate handed a connection by someone else:
#      AsyncDatabase::writer() returns &tokio_rusqlite::Connection, and
#      `conn.execute(sql, [])` needs no rusqlite:: path anywhere.
#      crates/orchestrator-security/src/secret_store_crypto.rs runs four
#      production SQL statements with zero rusqlite tokens — condition 1 reports
#      it clean. So the per-file residual of SQL statements and driver
#      references is frozen too.
#
# Usage:
#   persistence-dependency.rb                  verify the repository
#   persistence-dependency.rb --emit-baseline  print the candidate ledger
#   persistence-dependency.rb --emit-baseline --write   apply it locally

require "json"
require "optparse"
require "pathname"
require_relative "../lib/rust_source"
require_relative "../lib/ci_env"

# The same scanner core-boundary.rb and coordination-governance.rb use. Counting
# the same tree three ways produces three reviewed states that all look correct;
# stripping inline cfg(test) modules is the difference between core's 237 and its
# reviewed 200, and between this ledger's 55 and the 75 FR-136 was drafted from.
include RustSource

# The prose half of the scan's definition. It is compared against the ledger's
# copy below, which catches a ledger that stopped following the constant — and
# nothing more. Prose cannot say whether the constant describes what the code
# does; FR-139 found it claiming "its non-test Rust source" while the walk read
# only <member>/src, so five build scripts, two of them in `forbidden` crates,
# were governed by condition 1 and invisible to condition 2. `scanRoots` in the
# ledger is what observes the scan; this string is what a reader is owed.
SCOPE = "every workspace member listed in the root Cargo.toml, its Cargo.toml " \
  "parsed by dependency section, and its non-test Rust source outside core — " \
  "its src tree and its Cargo build script — excluding inline cfg(test) " \
  "modules, files under a tests directory, and files named test*.rs".freeze

# rusqlite and tokio-rusqlite both carry the driver. Freezing only the former
# leaves the async wrapper as an unguarded second door.
DRIVER = /\A(?:tokio-)?rusqlite\z/
DRIVER_KEY = /^\s*((?:tokio-)?rusqlite)\s*=/

# Uppercase only, and anchored to the opening quote of a string literal. A
# case-insensitive match reads "update", "create" and "delete" out of ordinary
# English in ordinary strings — the first draft of this gate found 26 statements
# in daemon where there are 19, and four in crates/orchestrator-config where
# there are none. Measured again for FR-139: relaxing to case-insensitive reads
# 20 help strings in crates/cli/src/commands/guide.rs as SQL.
#
# PRAGMA is the one verb FR-139 added, and the narrow shape is deliberately
# unchanged around it. It has two real hits and zero false ones —
# orchestrator-security/src/lib.rs (an `exempt` crate running SQL on a borrowed
# connection, exactly the shape condition 2 exists for) and
# slack-gateway/src/store.rs. VACUUM, BEGIN, COMMIT and WITH were measured the
# same way and rejected: every hit on this tree is a log message or prose
# (daemon/src/server/system.rs:140 and integration-tests/src/lib.rs:1600 both
# log "VACUUM"), so adding them would buy false positives and no statements.
#
# The anchor crosses a leading escape sequence as well as leading whitespace.
# `"\n            SELECT …"` is `"`, `\`, `n` in the source text, which `"\s*`
# cannot step over. There are zero such literals on this tree today, so this is
# closing a free bypass rather than repairing an undercount — the total is 114
# with or without the escape branch.
SQL_STATEMENT = /"(?:\\[nrt]|\s)*(?:SELECT|INSERT|UPDATE|DELETE|CREATE TABLE|CREATE INDEX|DROP|ALTER|REPLACE INTO|PRAGMA)\b/

# What each role permits. `forbidden` is the only role whose current state and
# target state differ: scheduler and daemon declare the driver today, and the
# residual below records exactly how much, so the declaration is tolerated while
# it is being paid down and condition 2 stops it from growing.
ROLES = {
  "persistence" => { declare: :dependencies, residual: true },
  "forbidden" => { declare: :residual_only, residual: true },
  "exempt" => { declare: :dependencies, residual: true },
  "separate-database" => { declare: :dependencies, residual: true },
  "test-only" => { declare: :dev_dependencies, residual: false },
  "none" => { declare: :never, residual: false }
}.freeze

options = {
  ledger: "config/governance/persistence-dependency-ledger.json",
  emit_baseline: false,
  write: false
}
OptionParser.new do |parser|
  parser.on("--ledger PATH") { |value| options[:ledger] = value }
  parser.on("--emit-baseline") { options[:emit_baseline] = true }
  parser.on("--write") { options[:write] = true }
end.parse!

repo_root = Pathname.new(File.expand_path("../..", __dir__))
ledger_path = repo_root.join(options[:ledger])

# The workspace members list, not a crates/* glob. core-boundary.rb globs
# crates/* plus core, so a member declared anywhere else — tools/, a nested
# workspace, a crate moved out of crates/ — is invisible to it. The authority on
# what a member is, is the [workspace] table.
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

# Parsed by section rather than by whole-file match. core-boundary.rb's
# `File.read(manifest).match?(...)` cannot tell a [dev-dependencies] entry from a
# [dependencies] one, which is why crates/integration-tests sits in its frozen
# list beside four production crates as though it were the same kind of fact.
def driver_declarations(manifest_path)
  return { "dependencies" => [], "devDependencies" => [] } unless manifest_path.file?

  section = nil
  found = { "dependencies" => [], "devDependencies" => [] }
  File.readlines(manifest_path).each do |line|
    if (header = line[/^\s*\[([^\]]+)\]\s*$/, 1])
      section = header
      next
    end
    match = line.match(DRIVER_KEY)
    next unless match

    case section
    when "dependencies", "build-dependencies" then found["dependencies"] << match[1]
    when "dev-dependencies" then found["devDependencies"] << match[1]
    when /\Atarget\..*\.dependencies\z/ then found["dependencies"] << match[1]
    when /\Atarget\..*\.dev-dependencies\z/ then found["devDependencies"] << match[1]
    end
  end
  found.transform_values { |names| names.uniq.sort }
end

# What the scan actually reads for one member: its src tree, and its Cargo build
# script.
#
# The build script is here because condition 1 already counts
# [build-dependencies] as a *production* declaration (see driver_declarations
# below). Reading the manifest half of build-time driver use while refusing to
# read the source half governs a usage the gate can never see: five members ship
# a build script, and crates/daemon and crates/orchestrator-scheduler — the two
# `forbidden` crates — are two of them. All five hold zero driver references and
# zero SQL today, so FR-139 closed a latent hole rather than a live one.
#
# The path is read from the manifest rather than assumed, because Cargo lets a
# package name it (`build = "custom.rs"`), and a member that renamed its build
# script would otherwise drop out of the scan silently.
def member_scan_roots(repo_root, member)
  manifest = repo_root.join(member, "Cargo.toml")
  script = "build.rs"
  if manifest.file?
    declared = File.read(manifest)[/^\s*build\s*=\s*"([^"]+)"/, 1]
    script = declared if declared
  end
  [repo_root.join(member, "src"), repo_root.join(member, script)].select(&:exist?)
end

# Every non-core member source file that either names the driver or executes SQL.
# The union matters: the two sets are not the same, and the difference is where
# a driver-token inventory goes blind.
#
# The roots come from the member list, not from RustSource.rust_source_files —
# that helper globs crates/*/src, so a member declared anywhere else has its
# manifest discovered by condition 1 and its source read by nobody. Only the
# counting is shared; the discovery is this gate's own.
def member_references(repo_root, scanned_members, roots)
  files = rust_files_under(repo_root, roots)

  files.each_with_object({}) do |path, collected|
    source = scannable_source(path)
    driver = source.scan(/rusqlite/).length
    sql = source.scan(SQL_STATEMENT).length
    next if driver.zero? && sql.zero?

    relative = relative_path(repo_root, path)
    member = scanned_members.select { |root| relative.start_with?("#{root}/") }.max_by(&:length)
    collected[relative] = { "crate" => member, "rusqlite" => driver, "sql" => sql }
  end.sort.to_h
end

# The reviewed half. Roles and categories are decisions; the emitter carries them
# through rather than inventing them, so --emit-baseline regenerates the counts a
# reviewer cannot check by hand and leaves the judgements a reviewer must make.
def reviewed_half(ledger_path)
  return { "decision" => {}, "roles" => {}, "categories" => {} } unless ledger_path.file?

  ledger = JSON.parse(File.read(ledger_path))
  {
    "decision" => ledger["decision"] || {},
    "roles" => ledger["roles"] || {},
    "categories" => (ledger["references"] || {}).transform_values { |entry| entry["category"] }
  }
end

def snapshot(repo_root, ledger_path)
  members = workspace_members(repo_root)
  reviewed = reviewed_half(ledger_path)
  declarations = members.to_h do |member|
    [member, driver_declarations(repo_root.join(member, "Cargo.toml"))]
  end
  scanned_members = members.reject { |member| member == "core" }
  roots = scanned_members.flat_map { |member| member_scan_roots(repo_root, member) }
  references = member_references(repo_root, scanned_members, roots).to_h do |file, entry|
    [file, entry.merge("category" => reviewed["categories"][file] || "unclassified")]
  end
  {
    "schemaVersion" => 2,
    "scope" => SCOPE,
    # The roots the walk actually visited, as opposed to the roots SCOPE says it
    # visits. Frozen in the ledger and compared by exact equality in both
    # directions, so narrowing the walk is a review event with a diff rather
    # than a silently smaller number. This is the counterpart the scope check
    # lacked: prose compared to prose cannot fail on a scan that stopped
    # matching its own description.
    "scanRoots" => roots.map { |root| relative_path(repo_root, root) }.sort,
    "decision" => reviewed["decision"],
    "roles" => members.to_h { |member| [member, reviewed["roles"][member] || { "role" => "unclassified" }] },
    "declarations" => declarations,
    "references" => references,
    "totals" => {
      "members" => members.length,
      "referencedFiles" => references.length,
      "rusqlite" => references.values.sum { |entry| entry["rusqlite"] },
      "sql" => references.values.sum { |entry| entry["sql"] }
    }
  }
end

# --- Condition 1: who may declare the driver ---------------------------------
def declaration_errors(snapshot)
  errors = []
  snapshot["roles"].each do |member, entry|
    role = entry["role"]
    rule = ROLES[role]
    if rule.nil?
      errors << "  #{member}: role #{role.inspect} is not one of #{ROLES.keys.join(', ')}; " \
        "a member with no reviewed role cannot be checked"
      next
    end

    declared = snapshot["declarations"][member] || {}
    production = declared["dependencies"] || []
    development = declared["devDependencies"] || []

    case rule[:declare]
    when :never
      unless production.empty? && development.empty?
        errors << "  #{member} is #{role} and must not name the SQLite driver at all, " \
          "but declares #{(production + development).join(', ')}"
      end
    when :dev_dependencies
      unless production.empty?
        errors << "  #{member} is #{role} and may name the driver only under " \
          "[dev-dependencies], but declares #{production.join(', ')} as a production dependency"
      end
    when :residual_only
      if !production.empty? && !entry["residualDeclaration"]
        errors << "  #{member} is #{role} with no recorded residual declaration, " \
          "but declares #{production.join(', ')}"
      end
    end
  end
  errors
end

# --- Condition 2: who may use it ---------------------------------------------
# Exact equality in both directions, not a monotonic ratchet. FR-128 found
# capturesOrJsonPath sitting at 54 against a reviewed 55: under a monotonic rule
# a decrease passes silently, and a decrease is the one event this ledger exists
# to record — it is the migration finishing.
def reference_errors(expected, actual)
  before = expected || {}
  after = actual || {}
  lines = []
  (after.keys - before.keys).sort.each do |file|
    entry = after[file]
    lines << "  + #{file} has #{entry['rusqlite']} driver reference(s) and " \
      "#{entry['sql']} SQL statement(s) and is not in the ledger"
  end
  (before.keys - after.keys).sort.each do |file|
    lines << "  - #{file} no longer touches persistence; the ledger still claims " \
      "#{before[file]['rusqlite']} driver reference(s) and #{before[file]['sql']} SQL statement(s)"
  end
  (before.keys & after.keys).sort.each do |file|
    %w[rusqlite sql].each do |field|
      next if before[file][field] == after[file][field]
      lines << "  ~ #{file} #{field} #{before[file][field]} -> #{after[file][field]}"
    end
    next if before[file]["crate"] == after[file]["crate"]
    lines << "  ~ #{file} moved from #{before[file]['crate']} to #{after[file]['crate']}"
  end
  lines
end

# Requirement 1 stated as an assertion rather than as prose. Every file the scan
# finds carries a reviewed category, so a file added to the tree cannot be
# absorbed as "already reviewed" — it arrives as `unclassified` and fails.
#
# There used to be a second branch here: sum the categorised references and
# require the total to equal the scanned total. It could not fail. `references`
# and `totals["rusqlite"]` are the same reduction over the same hash with no
# rewrite between them, so the comparison was the scan against itself; a file
# with no category was counted into the total and then found equal to it. FR-139
# removed it, because the branch's own acceptance test — produce an input that
# makes it fail — has no answer. Coverage was never lost: an unledgered file
# fails on reference_errors' exact equality, and an unreviewed one fails above.
def classification_errors(snapshot)
  errors = []
  unclassified = snapshot["references"].select { |_, entry| entry["category"] == "unclassified" }
  unless unclassified.empty?
    errors << "  #{unclassified.length} file(s) touch persistence with no reviewed category:\n" +
      unclassified.keys.sort.map { |file| "    #{file}" }.join("\n")
  end
  errors
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
  warn "persistence dependency ledger not found at #{options[:ledger]}"
  warn "the roles and categories in it are decisions; author them and commit the ledger"
  exit 1
end

expected = JSON.parse(File.read(ledger_path))
errors = []

# Two checks, and the first one alone is what FR-139 found insufficient.
#
# The scope check compares the ledger's copy of the prose to the constant. That
# catches a ledger left behind by an edit to SCOPE and nothing else: both sides
# are prose, so a constant that has stopped describing the scan reads as
# agreement. It was agreeing right up until FR-139, while the walk read only
# <member>/src and the prose said "its non-test Rust source".
#
# The scanRoots check compares the ledger's reviewed root list to the roots the
# scan just walked. That one has a real subject — narrowing the walk changes the
# list, and a reviewer reading the ledger sees crates/daemon/build.rs in it.
if expected["scope"] != SCOPE
  errors << "ledger scope prose does not match the scan this gate implements; " \
    "the ledger describes something the gate does not measure"
end

expected_roots = expected["scanRoots"] || []
if expected_roots.sort != actual["scanRoots"]
  detail = []
  added = actual["scanRoots"] - expected_roots
  removed = expected_roots - actual["scanRoots"]
  detail << "  + #{added.sort.join(', ')} is scanned and is not in the reviewed root list" unless added.empty?
  detail << "  - #{removed.sort.join(', ')} is in the reviewed root list and is no longer scanned" unless removed.empty?
  errors << "the roots this gate reads differ from the reviewed ledger:\n#{detail.join("\n")}"
end

if (expected["roles"] || {}).keys.sort != actual["roles"].keys.sort
  added = actual["roles"].keys - (expected["roles"] || {}).keys
  removed = (expected["roles"] || {}).keys - actual["roles"].keys
  detail = []
  detail << "  + #{added.sort.join(', ')} is a workspace member with no reviewed role" unless added.empty?
  detail << "  - #{removed.sort.join(', ')} is in the ledger and is no longer a member" unless removed.empty?
  errors << "workspace membership differs from the reviewed ledger:\n#{detail.join("\n")}"
end

declaration_lines = declaration_errors(actual)
unless declaration_lines.empty?
  errors << "crates naming the SQLite driver violate the reviewed chokepoint:\n#{declaration_lines.join("\n")}"
end

# Additions and removals are different events and are judged differently.
#
# An addition is a policy question, and the role rule above already answered it:
# freezing the set here as well would make the rule unreachable — every addition
# would fail on the freeze first, including the ones a role explicitly permits,
# and the gate would be a ratchet wearing a policy's diagnostics.
#
# A removal is not a policy question. It is the migration finishing, and it
# leaves the ledger asserting a dependency the tree no longer has — green, and
# false. FR-128 found capturesOrJsonPath sitting at 54 against a reviewed 55 for
# exactly that reason, so a removal has to be blessed rather than absorbed.
stale = (expected["declarations"] || {}).map do |member, entry|
  current = actual["declarations"][member]
  next if current.nil?

  gone = %w[dependencies devDependencies].flat_map do |section|
    (entry[section] || []) - (current[section] || [])
  end
  next if gone.empty?

  "  - #{member} no longer declares #{gone.sort.join(', ')}; the ledger still claims it"
end.compact
unless stale.empty?
  errors << "the ledger claims driver declarations the workspace no longer has:\n#{stale.join("\n")}"
end

reference_lines = reference_errors(expected["references"], actual["references"])
unless reference_lines.empty?
  errors << "persistence touch points differ from the reviewed ledger:\n#{reference_lines.join("\n")}"
end

classification_lines = classification_errors(actual)
unless classification_lines.empty?
  errors << "the classification does not cover the scan:\n#{classification_lines.join("\n")}"
end

if errors.empty?
  puts "Persistence dependency: PASS"
  puts "  decision: #{actual['decision']['form']} scoped to #{actual['decision']['database']}"
  counts = actual["roles"].values.group_by { |entry| entry["role"] }.transform_values(&:length)
  puts "  #{actual['totals']['members']} member(s): " +
    counts.sort.map { |role, count| "#{count} #{role}" }.join(", ")
  puts "  #{actual['totals']['rusqlite']} driver reference(s) and " \
    "#{actual['totals']['sql']} SQL statement(s) across " \
    "#{actual['totals']['referencedFiles']} file(s) outside core"
  exit 0
end

warn "Persistence dependency: FAIL"
errors.each { |error| warn "  #{error}" }
warn "  the decision is recorded in docs/design_doc/orchestrator/147-persistence-dependency-chokepoint.md"
warn "  regenerate the derived half with --emit-baseline, review the diff, and commit it " \
  "together with the change that caused it"
exit 1
