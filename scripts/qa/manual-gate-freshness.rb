#!/usr/bin/env ruby
# frozen_string_literal: true

# Reports how long ago each manual-runbook gate was actually run.
#
# Two different things live in this script, and only one of them can fail the
# build. That split is the point of FR-158 rather than an implementation detail.
#
#   Reported, never enforced: staleness. A gate nobody has run in 90 days is
#   listed and the exit status is unaffected. FR-158's subject is that the
#   governance surface has become the repository's largest source of work, and a
#   gate that goes red because a human has not followed a runbook lately would be
#   one more thing to feed — it would be answered by running the cheapest thing
#   that clears it, which is not the same as running the runbook.
#
#   Enforced: that this ledger and config/governance/qa-gate-surface.json agree
#   about which gates are manual-runbook. That is a fact about two committed
#   files, it is decidable, and it is what stops the report from quietly
#   narrowing. A gate reclassified to manual-runbook and never added here would
#   otherwise be missing from every report ever printed, and a report that omits
#   a gate looks exactly like a report where that gate is fresh (§4.4 shape 2).
#
# Usage:
#   manual-gate-freshness.rb            report, and fail only on set disagreement
#   manual-gate-freshness.rb --strict   also fail on stale entries (not used by CI)

require "json"
require "date"
require "pathname"

repo_root = Pathname.new(File.expand_path("../..", __dir__))
strict = ARGV.include?("--strict")

ledger_path = repo_root.join("config/governance/manual-gate-freshness.json")
manifest_path = repo_root.join("config/governance/qa-gate-surface.json")

[ledger_path, manifest_path].each do |path|
  next if path.file?

  warn "missing #{path.relative_path_from(repo_root)}"
  exit 1
end

ledger = JSON.parse(File.read(ledger_path))
manifest = JSON.parse(File.read(manifest_path))

declared = manifest["scripts"]
  .select { |entry| entry["enforcement"] == "manual-runbook" }
  .map { |entry| entry["path"] }
  .sort

# Fail closed on an empty read. This repository classifies 35 gates
# manual-runbook, so an empty set means the manifest could not be parsed the way
# this script expects — and zero rows and N passing rows are indistinguishable in
# an exit code.
if declared.empty?
  warn "the manifest declares no manual-runbook gates at all"
  warn "  35 are expected; an empty set is a broken read, not a clean result"
  exit 1
end

recorded = (ledger["gates"] || {}).keys.sort
errors = []

missing = declared - recorded
extra = recorded - declared
unless missing.empty?
  errors << "manual-runbook gate(s) absent from the freshness ledger:\n" +
            missing.map { |path| "  + #{path}" }.join("\n") +
            "\n  a gate missing here is missing from every report, which reads exactly" \
            "\n  like a gate that is fresh"
end
unless extra.empty?
  errors << "freshness ledger names gate(s) the manifest no longer classifies " \
            "manual-runbook:\n" + extra.map { |path| "  - #{path}" }.join("\n")
end

stale_after = ledger["staleAfterDays"] || 90
today = Date.today
rows = declared.map do |path|
  entry = (ledger["gates"] || {})[path] || {}
  last = entry["lastRun"]
  if last.nil?
    [path, nil, "never recorded"]
  else
    age = (today - Date.parse(last["date"])).to_i
    note = +"#{age}d ago at #{last["revision"].to_s[0, 8]}"
    note << " (exit #{last["exitStatus"]})" unless last["exitStatus"].to_i.zero?
    note << " (dirty worktree)" if last["worktreeDirty"]
    [path, age, note]
  end
end

stale = rows.select { |_, age, _| age.nil? || age > stale_after }

puts "Manual-runbook gate freshness (stale after #{stale_after} days)"
puts ""
rows.sort_by { |path, age, _| [age.nil? ? 0 : 1, -(age || 0), path] }.each do |path, age, note|
  marker = age.nil? || age > stale_after ? "STALE" : "  ok "
  puts format("  %-5s %-58s %s", marker, path.sub("scripts/qa/", ""), note)
end
puts ""
puts "#{stale.length} of #{rows.length} gate(s) stale or never recorded"

unless errors.empty?
  warn ""
  errors.each { |message| warn message }
  warn ""
  warn "regenerate the ledger's gate set from the manifest and commit both together"
  exit 1
end

if strict && !stale.empty?
  warn "--strict: #{stale.length} stale gate(s)"
  exit 1
end

puts "freshness report only; staleness does not fail this check"
exit 0
