#!/usr/bin/env ruby

# Freezes the `core` crate boundary against config/governance/core-boundary-ledger.json.
#
# FR-047 and FR-048 extracted orchestrator-config and orchestrator-scheduler, but
# core is still a god crate: 52 top-level `pub mod`, and a SQLite layer that is
# not a layer at all — 37 of its files reference rusqlite, and most of them
# interleave SQL with domain logic in the same function. FR-130 proposes
# extracting orchestrator-persistence. This gate is the step before that: it
# records the boundary as it stands so the extraction has a reviewed starting
# point and cannot silently grow while it waits.
#
# Usage:
#   core-boundary.rb                    verify the repository against the ledger
#   core-boundary.rb --emit-baseline    print the candidate ledger
#   core-boundary.rb --emit-baseline --write   apply it locally, then read the diff

require "json"
require "optparse"
require "pathname"
require_relative "../lib/rust_source"

# The scan is shared with scripts/qa/coordination-governance.rb. Both ledgers
# count the same tree, and stripping inline cfg(test) modules moves this ledger's
# rusqlite total from 237 to 200 — so the scanner is one file, not two lookalikes.
include RustSource

CORE_ROOT = "core/src/".freeze
# The item kinds FR-130 counted, plus `pub async fn` which it missed. `pub(crate)`
# is deliberately not matched: it is not crate-external surface.
PUBLIC_ITEM = /^\s*pub (?:async )?(?:fn|struct|enum|trait|type|const) /
SCOPE = "non-test Rust source under core/src, excluding inline cfg(test) modules, " \
  "files under a tests directory, and files named test*.rs; crate dependents are " \
  "read from the member Cargo.toml manifests".freeze

options = {
  ledger: "config/governance/core-boundary-ledger.json",
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

def core_source_files(repo_root)
  rust_source_files(repo_root).select do |path|
    path.extname == ".rs" && relative_path(repo_root, path).start_with?(CORE_ROOT)
  end
end

# `pub mod` in lib.rs is the crate's module surface; publicItems is every
# exported item across core. Both are counted after stripping cfg(test) modules,
# so a test helper marked `pub` does not read as public API.
def core_surface(repo_root, files)
  lib = strip_test_modules(File.read(repo_root.join("core/src/lib.rs")))
  {
    "files" => files.length,
    "pubMod" => lib.scan(/^pub mod /).length,
    "publicItems" => files.sum { |path| scannable_source(path).scan(PUBLIC_ITEM).length }
  }
end

# The per-file map is the ratchet and the extraction work-list at once. FR-130's
# own requirement 2 named fourteen files; the real inventory is this one, and
# recording it per file means the extraction can be checked off against something
# machine-readable rather than against prose.
def rusqlite_touch_points(repo_root, files)
  per_file = {}
  files.each do |path|
    count = scannable_source(path).scan(/rusqlite/).length
    next if count.zero?
    per_file[relative_path(repo_root, path)] = count
  end
  {
    "total" => per_file.values.sum,
    "files" => per_file.sort.to_h
  }
end

# core is not the persistence chokepoint the FR assumed: six crates take rusqlite
# directly. Freezing the list makes a seventh a review event rather than a
# discovery made during the extraction.
def rusqlite_dependent_crates(repo_root)
  manifests = [repo_root.join("core/Cargo.toml")]
  manifests.concat(Dir[repo_root.join("crates/*/Cargo.toml").to_s].map { |path| Pathname.new(path) })
  manifests.select(&:file?).select do |manifest|
    File.read(manifest).match?(/^\s*(?:tokio-)?rusqlite\s*=/)
  end.map { |manifest| relative_path(repo_root, manifest.dirname) }.sort
end

def boundary_snapshot(repo_root)
  files = core_source_files(repo_root)
  {
    "schemaVersion" => 1,
    "scope" => SCOPE,
    "coreSurface" => core_surface(repo_root, files),
    "rusqlite" => rusqlite_touch_points(repo_root, files),
    "rusqliteDependentCrates" => rusqlite_dependent_crates(repo_root)
  }
end

def surface_report(expected, actual)
  %w[files pubMod publicItems].map do |field|
    next if expected[field] == actual[field]
    "  ~ coreSurface.#{field} #{expected[field].inspect} -> #{actual[field].inspect}"
  end.compact
end

def rusqlite_report(expected, actual)
  before = expected["files"] || {}
  after = actual["files"] || {}
  lines = []
  (after.keys - before.keys).sort.each do |file|
    lines << "  + #{file} references rusqlite #{after[file]} time(s) and is not in the ledger"
  end
  (before.keys - after.keys).sort.each do |file|
    lines << "  - #{file} no longer references rusqlite; the ledger still claims #{before[file]}"
  end
  (before.keys & after.keys).sort.each do |file|
    next if before[file] == after[file]
    lines << "  ~ #{file} #{before[file]} -> #{after[file]} rusqlite reference(s)"
  end
  if expected["total"] != actual["total"]
    lines << "  ~ rusqlite.total #{expected["total"].inspect} -> #{actual["total"].inspect}"
  end
  lines
end

actual = boundary_snapshot(repo_root)

if options[:emit_baseline]
  if options[:write]
    # A regenerated ledger is a proposal for a human to read in a diff. In CI
    # there is no human, and an automatic rewrite would turn the review gate into
    # decoration.
    if ENV.key?("CI")
      warn "refusing --write under CI: a regenerated ledger must be reviewed by a human"
      warn "run --emit-baseline locally, read the diff, and commit the ledger with the change"
      exit 2
    end
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
  warn "core boundary ledger not found at #{options[:ledger]}"
  warn "generate it with --emit-baseline --write and commit it"
  exit 1
end

expected = JSON.parse(File.read(ledger_path))
errors = []

if expected["scope"] != SCOPE
  errors << "ledger scope prose does not match the scan this gate implements; " \
    "the ledger describes something the gate does not measure"
end

# Exact equality, not the monotonic ratchet FR-130 asked for. A count that drops
# below its baseline leaves the ledger asserting debt the repository no longer
# carries, and the gate stays green while saying something false — FR-128 found
# capturesOrJsonPath sitting at 54 against a reviewed 55 for exactly that reason.
# Here a decrease is the goal, which makes blessing it the interesting review
# event; --emit-baseline makes the blessing cost one command.
surface_lines = surface_report(expected["coreSurface"] || {}, actual["coreSurface"])
unless surface_lines.empty?
  errors << "core public surface differs from the reviewed ledger:\n#{surface_lines.join("\n")}"
end

rusqlite_lines = rusqlite_report(expected["rusqlite"] || {}, actual["rusqlite"])
unless rusqlite_lines.empty?
  errors << "core rusqlite touch points differ from the reviewed ledger:\n#{rusqlite_lines.join("\n")}"
end

if expected["rusqliteDependentCrates"] != actual["rusqliteDependentCrates"]
  added = actual["rusqliteDependentCrates"] - (expected["rusqliteDependentCrates"] || [])
  removed = (expected["rusqliteDependentCrates"] || []) - actual["rusqliteDependentCrates"]
  detail = []
  detail << "  + #{added.join(", ")}" unless added.empty?
  detail << "  - #{removed.join(", ")}" unless removed.empty?
  errors << "crates depending directly on rusqlite differ from the reviewed ledger:\n#{detail.join("\n")}"
end

if errors.empty?
  puts "Core boundary: PASS"
  puts "  core/src files: #{actual["coreSurface"]["files"]}, " \
    "pub mod: #{actual["coreSurface"]["pubMod"]}, " \
    "public items: #{actual["coreSurface"]["publicItems"]}"
  puts "  rusqlite: #{actual["rusqlite"]["total"]} reference(s) across " \
    "#{actual["rusqlite"]["files"].length} file(s) in core, " \
    "#{actual["rusqliteDependentCrates"].length} crate(s) depend on it directly"
  exit 0
end

warn "Core boundary: FAIL"
errors.each { |error| warn "  #{error}" }
warn "  regenerate with --emit-baseline, review the diff, and commit the ledger " \
  "together with the change that caused it"
exit 1
