#!/usr/bin/env ruby
# frozen_string_literal: true

# Reports how long ago each manual-runbook gate was actually run.
#
# Two different things live in this script, and only one of them can fail the
# build. That split is the point of FR-158 rather than an implementation detail.
#
#   Reported, not enforced *here*: staleness. A gate nobody has run in 90 days
#   is listed and the exit status of a bare run is unaffected. FR-158's subject
#   is that the governance surface has become the repository's largest source of
#   work, and a gate that goes red on every push because a human has not
#   followed a runbook lately would be one more thing to feed — it would be
#   answered by running the cheapest thing that clears it, which is not the same
#   as running the runbook.
#
#   FR-165 keeps that split and adds one enforcement point at the far end:
#   --strict runs in the release workflow, so stale evidence blocks a release
#   without blocking a push. The daily cost stays zero and the ledger finally
#   drives something.
#
#   Freshness is not recency alone. See the criterion below: a record counts
#   only if the run it records succeeded on a clean tree.
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
#   manual-gate-freshness.rb --strict   also fail on gates that are not fresh
#                                       (release.yml, not ci.yml)

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

# Fail closed on an empty read: zero rows and N passing rows are
# indistinguishable in an exit code.
#
# The expected count is *derived*, never restated. This diagnostic used to say
# "35 are expected" and was carrying that number while the manifest had moved to
# 38 — the count is exactly the thing this file exists to let move, so a literal
# here is stale the first time a gate is reclassified (§4.4 shape 7: derive the
# expected value from the ledger, never restate it). The ledger is the right
# second source precisely because the set-agreement check below is what keeps it
# honest: if the two files ever disagree, that check fails and says so.
if declared.empty?
  expected = (ledger["gates"] || {}).length
  warn "the manifest declares no manual-runbook gates at all"
  warn "  the freshness ledger records #{expected}; an empty set is a broken read, not a clean result"
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

# Which gates can block a release.
#
# --strict runs in release.yml, and some manual gates cannot be a release
# precondition however much one would like them to be. scripts/watchdog.sh is
# the clearest: its own manifest entry describes an unbounded foreground loop
# that overwrites target/release/orchestratord, so "run it before every release"
# asks a human to start an infinite loop that clobbers the artifact being
# released. A blocking gate nobody can satisfy is not enforcement, it is a
# thing people learn to route around, and a routed-around gate is worse than an
# advisory one because it still reads as enforcement.
#
# So: `releaseBlocking: false`, per gate, with a reason. The shape rules this
# obeys, because exemption mechanisms are where §4.4 finds most of its material:
#
#   - Per-gate keys only. There is no pattern, prefix or subtree form, because
#     shape 8 is exactly that: a `skip-tree` absorbs instances that do not exist
#     yet and never produces a line in any log. An exemption here can only ever
#     name one gate that already exists.
#   - It cannot outlive what it excuses. These are keys in `gates`, and the
#     set-agreement check above forces that map to equal the manifest's
#     manual-runbook set — so a retired gate's exemption is a hard error rather
#     than a line that lingers.
#   - A reason is mandatory and an orphaned reason is an error. `--deny
#     unmatched-skip` was measured at FR-133 to cover less than its name
#     suggests; the lesson taken was that an exemption ratchet nobody has tried
#     to trip is one whose reach you are guessing at. Both directions are
#     asserted below and both have fixtures.
#   - Exempt gates are printed, always, with their reason. An exemption that
#     does not appear in the output is the enumeration failure wearing a
#     different hat.
release_exempt = {}
(ledger["gates"] || {}).each do |path, entry|
  blocking = entry["releaseBlocking"]
  reason = entry["releaseBlockingReason"]

  unless blocking.nil? || [true, false].include?(blocking)
    errors << "#{path}: releaseBlocking must be true or false, got #{blocking.inspect}"
    next
  end

  if blocking == false
    if reason.nil? || reason.to_s.strip.empty?
      errors << "#{path}: releaseBlocking is false with no releaseBlockingReason\n" \
                "  an exemption without a reason is one nobody can ever retire"
    else
      release_exempt[path] = reason
    end
  elsif !(reason.nil? || reason.to_s.strip.empty?)
    errors << "#{path}: carries releaseBlockingReason but is release-blocking\n" \
              "  delete the reason with the exemption; a reason left behind is the\n" \
              "  next reader's evidence for an exemption that is no longer there"
  end
end

stale_after = ledger["staleAfterDays"] || 90
today = Date.today

# What counts as a run.
#
# Recency was once the whole criterion: `age.nil? || age > stale_after`, with
# exitStatus and worktreeDirty printed beside the row but not consulted. That
# asks a different question from the one this ledger is for. The subject is
# "has this runbook been exercised, and did exercising it establish anything",
# and a record whose exitStatus is 1 answers the first half yes and the second
# half no while reading `ok` in every report — §4.4 shape 6, a status field
# reporting something other than what you are asking. Measured when this was
# written: test-attention-inbox.sh carried exitStatus 1 dated 2026-08-11 and
# printed `ok`, and --strict passed it too.
#
# worktreeDirty voids a record for the same reason §4.6 condition 1 voids a
# certification run: a gate exercised against uncommitted edits did not observe
# the committed tree, so whatever it established was about a state that is not
# in the repository. Both are recorded by scripts/lib/gate_runlog.sh precisely
# so that something can act on them; until now nothing did.
#
# Each failing branch is labelled distinctly rather than collapsed into STALE,
# so the report says *which way* a gate is unfresh. An operator's response to
# `failed` (read the log, fix the gate) is not their response to `aged` (run
# the runbook), and a single marker for both hides that.
rows = declared.map do |path|
  entry = (ledger["gates"] || {})[path] || {}
  last = entry["lastRun"]
  if last.nil?
    [path, nil, "never recorded", :never]
  else
    age = (today - Date.parse(last["date"])).to_i
    note = +"#{age}d ago at #{last["revision"].to_s[0, 8]}"
    exit_status = last["exitStatus"].to_i
    note << " (exit #{exit_status})" unless exit_status.zero?
    note << " (dirty worktree)" if last["worktreeDirty"]

    reason =
      if !exit_status.zero? then :failed
      elsif last["worktreeDirty"] then :dirty
      elsif age > stale_after then :aged
      end
    [path, age, note, reason]
  end
end

stale = rows.reject { |_, _, _, reason| reason.nil? }

MARKERS = { never: "never", failed: "FAILED", dirty: "dirty", aged: "aged" }.freeze

puts "Manual-runbook gate freshness (stale after #{stale_after} days)"
puts ""
puts "  a gate is fresh only when its last recorded run succeeded (exit 0) on a"
puts "  clean worktree within #{stale_after} days; the other three states are named"
puts ""
rows.sort_by { |path, age, _, _| [age.nil? ? 0 : 1, -(age || 0), path] }
    .each do |path, _age, note, reason|
  marker = reason.nil? ? "ok" : MARKERS.fetch(reason)
  exempt = release_exempt.key?(path) ? "  [not release-blocking]" : ""
  puts format("  %-6s %-58s %s%s", marker, path.sub("scripts/qa/", ""), note, exempt)
end
puts ""

unless release_exempt.empty?
  puts "#{release_exempt.length} gate(s) do not block a release:"
  release_exempt.sort.each { |path, reason| puts "  - #{path}\n      #{reason}" }
  puts ""
end

# `each_with_object` rather than `filter_map`: macOS ships Ruby 2.6 and
# filter_map arrived in 2.7 (the same note persistence-dependency.rb carries).
by_reason = stale.group_by { |_, _, _, reason| reason }
breakdown = MARKERS.keys.each_with_object([]) do |reason, acc|
  count = by_reason[reason]
  acc << "#{count.length} #{reason}" if count
end
puts "#{stale.length} of #{rows.length} gate(s) not fresh" +
     (breakdown.empty? ? "" : " (#{breakdown.join(', ')})")

unless errors.empty?
  warn ""
  errors.each { |message| warn message }
  warn ""
  warn "regenerate the ledger's gate set from the manifest and commit both together"
  exit 1
end

blocking_stale = stale.reject { |path, _, _, _| release_exempt.key?(path) }

if strict && !blocking_stale.empty?
  warn ""
  warn "--strict: #{blocking_stale.length} release-blocking gate(s) not fresh"
  blocking_stale.sort_by { |path, _, _, _| path }.each do |path, _age, note, reason|
    warn format("  %-6s %-58s %s", MARKERS.fetch(reason), path, note)
  end
  warn ""
  warn "run the gate's owner runbook and commit the ledger, or — if it genuinely"
  warn "cannot be a release precondition — give it releaseBlocking: false with a"
  warn "releaseBlockingReason saying why."
  exit 1
end

if strict
  # Reaching here under --strict means blocking_stale was empty, which is the
  # thing worth saying. The old line said "staleness does not fail this check"
  # unconditionally, which is exactly false when --strict is what release.yml
  # passes: a reader checking whether the release gate is armed would have been
  # told it is not.
  puts "--strict: every release-blocking gate is fresh " \
       "(#{release_exempt.length} exempt, each with a reason above)"
else
  puts "freshness report only; staleness does not fail this check " \
       "(release.yml runs this with --strict, which does)"
end
exit 0
