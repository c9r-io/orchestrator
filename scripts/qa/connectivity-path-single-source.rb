#!/usr/bin/env ruby

# FR-163: every name in the daemon runtime layout is spelled in exactly one place.
#
# Before FR-163 the data directory was derived independently in four places, and
# `orchestrator.sock`, `agent_orchestrator.db` and `daemon.pid` were each spelled
# in two — split across crates that cannot see one another, so nothing in the tree
# ever compared them. Three had already drifted apart:
#
#   * a layout heuristic read a parent directory named `data` as a nested layout,
#     sending SecretStore reads and writes to different directories;
#   * the filesystem watcher's data-directory skip read `ORCHESTRATORD_DATA_DIR`
#     directly, so it did nothing in the default deployment where that variable
#     is unset;
#   * the daemon wrote a client bundle to `~/.orchestrator/control-plane/config.yaml`
#     while transport auto-discovery looked in `~/.orchestratord/...` — one
#     character apart, so that discovery branch had never once fired.
#
# The subject here is deliberately the **spelling of the layout**, not "who reads
# ambient state". An earlier draft asserted the latter and flagged four files that
# read `$HOME` to expand a leading tilde in a user-supplied path — true statements
# about ambient reads, and nothing to do with where the daemon keeps its socket.
# Widening a matcher until it covers what you meant is how it starts covering what
# you did not (§4.4 shape 10). What actually matters is that two pieces of code
# cannot disagree about a filename, and that is what this measures.
#
# This gate is a **count**, and per §4.4 a count may only ever be an additional
# condition. The behaviour is carried by the unit tests in
# `orchestrator_config::paths`, `secret_store_crypto` and `fs_watcher`, and by
# `scripts/qa/test-stale-socket-discovery.sh`. What this adds is the one thing
# those cannot see: a *second* spelling appearing somewhere none of them look.
# That is exactly the FR-163 defect shape — code that is individually reasonable
# and collectively inconsistent — and it produces no failing test on the day it
# is written.
#
# ## Scope, and why two copies of every file are read
#
# Scope is derived, never listed: every tracked non-test Rust file, with inline
# `#[cfg(test)]` modules stripped by `RustSource` (brace-matched over lexically
# masked source, so a brace inside a string literal cannot swallow the rest of the
# file). The 7 nested-`data/` database paths and the 3 test control-plane joins
# live in such modules and are correctly invisible here.
#
# Every subject here *is* a string literal, and masking blanks string literals —
# reading only the masked copy would measure nothing at all. Reading only the raw
# copy would count prose, the failure DD-142 recorded, and this file's own header
# names every one of these strings. Masking preserves line offsets, so both copies
# are read: the masked line decides whether the line is code, the raw line at the
# same index supplies the text. Prose is never counted and literals are never lost.
#
# ## Known limit, checked rather than inherited
#
# `rust_source.rb` excludes files by the basename pattern `test*.rs`, hiding two
# production modules from every ledger built on it (see
# docs/ticket/20260811-rust-source-test-basename-hides-production.md). Rather than
# inherit that silently, the final check below rescans with the exclusion lifted
# and fails if it finds a spelling the main scan could not see.
#
# Usage:
#   connectivity-path-single-source.rb                  verify against the allowlist
#   connectivity-path-single-source.rb --emit-baseline  print what the tree has now

require "json"
require "optparse"
require "pathname"
require_relative "../lib/rust_source"

REPO_ROOT = Pathname.new(File.expand_path("../..", __dir__))

# One entry per fact about the layout. Each pattern is matched against raw source
# at a position the masked copy has established is code.
LAYOUT_NAMES = {
  "data dir name" => /"\.orchestratord"/,
  "socket file name" => /"orchestrator\.sock"/,
  "database file name" => /"agent_orchestrator\.db"/,
  "pid file name" => /"daemon\.pid"/,
  "control-plane dir name" => /"control-plane"/,
  "client dir name" => /"\.orchestrator"/,
  "data dir env var" => /"ORCHESTRATORD_DATA_DIR"/,
}.freeze

CANON = "crates/orchestrator-config/src/paths.rs".freeze

# Files permitted to spell a given name, and why. `paths.rs` owns every one of
# them; anything else needs a reason that is not "it was convenient".
ALLOWLIST = {
  "data dir name" => { CANON => "the definition" },
  "socket file name" => { CANON => "the definition" },
  "database file name" => { CANON => "the definition" },
  "pid file name" => {
    CANON => "the definition",
    "crates/orchestrator-runner/src/runner/policy.rs" =>
      "not a path: a substring guard that refuses an agent command shaped like " \
      "`kill $(cat .../daemon.pid)`. It matches the daemon's pidfile by name " \
      "because the name is what appears in the command being screened.",
  },
  "control-plane dir name" => { CANON => "the definition" },
  "client dir name" => { CANON => "the definition" },
  "data dir env var" => { CANON => "the definition" },
}.freeze

options = { emit_baseline: false }
OptionParser.new do |opts|
  opts.on("--emit-baseline") { options[:emit_baseline] = true }
end.parse!

# Every tracked Rust file under the workspace source roots with the basename
# exclusion lifted — used only to prove that exclusion hides nothing from this
# gate.
def all_rust_files
  roots = [REPO_ROOT.join("core/src")]
  roots.concat(Dir[REPO_ROOT.join("crates/*/src").to_s].map { |p| Pathname.new(p) })
  roots.flat_map { |root| Dir[root.join("**/*.rs").to_s] }.map { |p| Pathname.new(p) }
end

def scan(files)
  found = Hash.new { |h, k| h[k] = Hash.new { |g, j| g[j] = [] } }
  files.each do |path|
    next unless path.extname == ".rs"

    masked = RustSource.masked_scannable_source(path).to_s
    raw = RustSource.scannable_source(path).to_s
    next if masked.empty?

    masked_lines = masked.lines
    relative = RustSource.relative_path(REPO_ROOT.to_s, path)
    raw.lines.each_with_index do |raw_line, index|
      # A line whose masked counterpart is blank is a comment or pure prose.
      next if masked_lines[index].to_s.strip.empty?

      LAYOUT_NAMES.each do |label, pattern|
        next unless raw_line.match?(pattern)

        found[label][relative] << [index + 1, raw_line.strip]
      end
    end
  end
  found
end

files = RustSource.rust_source_files(REPO_ROOT)
# Empty input fails closed: zero scanned files and a clean scan are different
# facts, and only one of them is evidence (§4.4 shape 5).
if files.empty?
  warn "    no Rust source files found; the scan read nothing"
  exit 1
end

found = scan(files)

if options[:emit_baseline]
  puts JSON.pretty_generate(
    found.transform_values do |by_file|
      by_file.transform_values { |hits| hits.map { |(n, text)| "#{n}: #{text}" } }
    end
  )
  exit 0
end

failures = []

LAYOUT_NAMES.each_key do |label|
  permitted = ALLOWLIST.fetch(label, {})
  actual = found[label]

  (actual.keys - permitted.keys).sort.each do |path|
    failures << "#{label}: spelled in #{path}, which is not permitted to spell it:\n" +
      actual[path].map { |(n, text)| "      #{n}: #{text}" }.join("\n") +
      "\n      derive it from #{CANON} instead"
  end

  # The mirror condition. A permitted site matching nothing is not harmless: it
  # means the scan stopped seeing a file it is supposed to watch — a moved
  # module, a renamed crate, a pattern that quietly stopped matching — and
  # without this the gate would report success having looked at nothing.
  (permitted.keys - actual.keys).sort.each do |path|
    failures << "#{label}: #{path} is allowlisted but the scan found no spelling there; " \
      "either the code moved (and this gate is now blind to it) or the entry is stale"
  end
end

# The basename-exclusion limit, re-derived on every run rather than asserted once
# in a comment.
wider = scan(all_rust_files)
LAYOUT_NAMES.each_key do |label|
  hidden = wider[label].keys - found[label].keys - ALLOWLIST.fetch(label, {}).keys
  next if hidden.empty?

  failures << "#{label}: rust_source.rb's test*.rs basename exclusion hides a spelling " \
    "in #{hidden.sort.join(', ')} from this gate"
end

if failures.empty?
  puts "connectivity path single-source: #{LAYOUT_NAMES.size} layout names, each spelled only where permitted"
  LAYOUT_NAMES.each_key do |label|
    sites = found[label].keys.sort
    puts "  #{label}: #{sites.join(', ')}"
  end
  puts "scanned #{files.size} non-test Rust files"
  exit 0
end

failures.each { |line| warn "    #{line}" }
warn "    (scanned #{files.size} non-test Rust files)"
exit 1
