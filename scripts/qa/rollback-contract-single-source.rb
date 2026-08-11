#!/usr/bin/env ruby

# FR-165 requirement 2: the forward-only rollback contract is stated in one
# place, and every live restatement of it cites that place.
#
# The contract — migrations are forward-only, the previous release binary must be
# able to serve the current schema, restore is for disaster only — was stated in
# fifteen documents and asserted nowhere. The cost was measured, not imagined:
# `c1060338` centralised daemon readiness on a `--wait-ready` flag the 0.5.0
# binary cannot accept, and the behavioural half of clause 2 was dead from
# 2026-08-11 until `77cc351a`. Four manual gates went red and nothing else in the
# tree noticed. Prose restated fifteen times did not survive one flag.
#
# Clause 2 now has a mechanical guard of its own
# (`previous_release_schema_is_a_subset_of_current` in
# `core/src/persistence/schema_snapshot.rs`). This gate is the other half: it
# keeps the documents that state the contract pointing at the code that defines
# it, so the next reader finds one rule instead of fifteen paraphrases that can
# drift apart.
#
# ## Why the ledger is keyed per site and not per path
#
# `forward-only` is four different concepts in this repository, and the reason
# this is not merely "classify before counting" is where they sit:
#
#   A       the daemon migration rollback contract — the subject here
#   B       the Slack Gateway's own schema, a different database
#   C       artifact forwarding in orchestrator-collab, nothing to do with schema
#   D       monotonic *state* change — connection generation/version CAS
#
# D lives at `docs/security/slack-gateway-threat-model.md` row T8, **four rows
# above the A-class row T12 in the same markdown table**. So there is no
# file-level or section-level scope predicate that separates A from D. A ledger
# keyed by path would classify that whole file as A, satisfy the citation
# requirement file-wide, and bless T8 in silence — §4.4 shape 3, a whole-file
# total standing in for the per-object association the contract claims. Sites are
# therefore keyed by a digest of the matched line.
#
# Digest rather than line number, because line numbers move and the governed
# prose moving is exactly what this gate exists to permit. A stale line number
# either points at innocent text or points past the end of the file; a stale
# digest simply stops matching, which is a fact this gate can report.
#
# ## Scope: no predicate at all
#
# Every tracked file whose bytes contain no NUL. Not `*.md` and `*.rs`: that
# reads as an obvious narrowing and it is §4.4 shape 9's third premise — a scope
# sufficient for today's tree is a fact about the tree, not about the check.
# Measured at `87af47a5`: the extension-free scan reads 1588 text files and finds
# the same 38 sites the two-extension scan finds, so the widest scope costs
# nothing and cannot be wrong later.
#
# Known limit, checked rather than assumed: 58 tracked paths are not regular
# files. All 58 are the `.agents/skills/` and `.cursor/skills/` mirror symlinks
# pointing into `.claude/skills/`, whose 90 files are tracked and scanned
# directly, so the real content is read once and nothing is hidden. The check at
# the bottom re-derives that rather than trusting this paragraph.
#
# Usage:
#   rollback-contract-single-source.rb                  verify against the ledger
#   rollback-contract-single-source.rb --emit-baseline  print what the tree has now

require "digest"
require "json"
require "optparse"
require "pathname"
require "set"

REPO_ROOT = Pathname.new(File.expand_path("../..", __dir__))
LEDGER_REL = "config/governance/rollback-contract-sites.json"
PATTERN = /forward[- ]only/i

# One entry per class, and the prose is load-bearing: the classification is a
# judgement a script cannot make, so the ledger records who decided and why.
# `cites` says whether a site of this class must point at the single source.
CLASSES = {
  "source" => { cites: false, summary: "the single source itself" },
  "A" => { cites: true, summary: "a live statement of the daemon migration rollback contract" },
  "record" => { cites: false, summary: "a changelog entry; a record of a change, not a statement of the rule" },
  "B" => { cites: false, summary: "the Slack Gateway's own schema — a different database" },
  "C" => { cites: false, summary: "unrelated to schema migration" },
  "D" => { cites: false, summary: "monotonic state change, not schema" },
  "index" => { cites: false, summary: "an index or table-of-contents row" },
  "meta" => { cites: false, summary: "governance prose whose subject is the contract" },
}.freeze

options = { emit_baseline: false }
OptionParser.new do |parser|
  parser.on("--emit-baseline", "print the sites the tree has now") { options[:emit_baseline] = true }
end.parse!

def text_files(root)
  tracked = `git -C #{root} ls-files -z`.split("\0")
  regular, irregular = tracked.partition { |rel| File.file?(root.join(rel)) }
  # `binread` returns nil for an empty file, and an empty file is text.
  text = regular.reject { |rel| root.join(rel).binread(8192).to_s.include?("\0") }
  [text, irregular]
end

def scan(root, files)
  sites = []
  files.each do |rel|
    root.join(rel).read.each_line.with_index do |line, index|
      next unless line.valid_encoding? && line =~ PATTERN

      stripped = line.strip
      sites << {
        "path" => rel,
        "line" => index + 1,
        "digest" => Digest::SHA256.hexdigest(stripped)[0, 12],
        "text" => stripped,
      }
    end
  rescue ArgumentError
    # An invalid byte sequence is not a site; it is also not a reason to stop.
    next
  end
  sites
end

# Every line of a file, by digest, so a citation can be checked against the file
# it is supposed to live in without caring where in the file it moved to.
def digests_of(root, rel)
  root.join(rel).read.each_line.map { |line| Digest::SHA256.hexdigest(line.strip)[0, 12] }.to_set
rescue ArgumentError
  Set.new
end

text, irregular = text_files(REPO_ROOT)

# Empty input fails closed. Zero scanned files and a clean scan are different
# facts and only one of them is evidence (§4.4 shape 5).
if text.empty?
  warn "    no tracked text files found; the scan read nothing"
  exit 1
end

sites = scan(REPO_ROOT, text)

if options[:emit_baseline]
  puts JSON.pretty_generate(sites)
  exit 0
end

ledger_path = REPO_ROOT.join(LEDGER_REL)
unless ledger_path.file?
  warn "    #{LEDGER_REL} is missing; there is nothing to check the tree against"
  exit 1
end
ledger = JSON.parse(ledger_path.read)

canon = ledger["singleSource"].to_s
failures = []

if canon.empty? || !REPO_ROOT.join(canon).file?
  failures << "singleSource #{canon.inspect} does not name a file that exists;\n" \
              "      every class-A citation points at it, so a missing single source makes them all vacuous"
end

booked = {}
(ledger["sites"] || []).each do |entry|
  path = entry["path"].to_s
  digest = entry["digest"].to_s
  klass = entry["class"].to_s
  key = [path, digest]

  if booked.key?(key)
    failures << "#{path} #{digest}: booked twice"
    next
  end
  booked[key] = entry

  unless CLASSES.key?(klass)
    failures << "#{path} #{digest}: class #{klass.inspect} is not one of #{CLASSES.keys.join(', ')}"
    next
  end

  cites = CLASSES.fetch(klass)[:cites]
  cited_by = entry["citedBy"].to_s

  if cites && cited_by.empty?
    failures << "#{path} #{digest}: class #{klass} must name the line that cites #{canon} in citedBy"
  elsif !cites && !cited_by.empty?
    # The orphaned-reason discipline the manual-gate exemption mechanism
    # settled on: a justification left behind after the thing it justified is
    # the next reader's evidence for a rule that is no longer there.
    failures << "#{path} #{digest}: class #{klass} does not require a citation but carries citedBy;\n" \
                "      delete it, or reclassify the site"
  end
end

whole_file = {}
(ledger["wholeFile"] || []).each do |entry|
  path = entry["path"].to_s
  klass = entry["class"].to_s
  reason = entry["reason"].to_s

  unless CLASSES.key?(klass)
    failures << "#{path}: whole-file class #{klass.inspect} is not one of #{CLASSES.keys.join(', ')}"
    next
  end
  if CLASSES.fetch(klass)[:cites]
    failures << "#{path}: class #{klass} requires a per-site citation and cannot be booked whole-file;\n" \
                "      a file-level entry is exactly the association this gate refuses to accept"
    next
  end
  if reason.strip.empty?
    failures << "#{path}: booked whole-file with no reason;\n" \
                "      an allowance nobody can retire is one that outlives what it was for"
    next
  end
  whole_file[path] = entry
end

# ── Every site in the tree must be accounted for ──────────────────────────────
unclassified = sites.reject { |site| booked.key?([site["path"], site["digest"]]) || whole_file.key?(site["path"]) }
unclassified.each do |site|
  failures << "#{site['path']}:#{site['line']} is not classified in #{LEDGER_REL}\n" \
              "      #{site['digest']}  #{site['text'][0, 90]}\n" \
              "      book it as one of #{CLASSES.keys.join(', ')}; if it states the contract it is class A\n" \
              "      and must cite #{canon}"
end

# ── The mirror condition ─────────────────────────────────────────────────────
# A booked site matching nothing is not harmless. It means the gate stopped
# seeing prose it is supposed to watch — reworded, moved to another file, or
# deleted — and without this the gate reports success having checked less than it
# claims. This is the branch that catches the gate going blind rather than the
# tree going wrong, and eight of the nine recorded fixture-target drifts stayed
# green precisely because nothing occupied it.
present = sites.map { |site| [site["path"], site["digest"]] }.to_set
pruned = []
booked.each do |(path, digest), entry|
  next if present.include?([path, digest])

  if entry["ephemeral"]
    pruned << "#{path} #{digest} (#{entry['class']})"
    next
  end

  failures << "#{path} #{digest} is booked but no line in the tree matches it\n" \
              "      either the statement was reworded or moved — reclassify it and this gate stays honest —\n" \
              "      or it is gone, in which case delete the entry. Until then this gate is blind to that site."
end

paths_present = sites.map { |site| site["path"] }.to_set
whole_file.each do |path, entry|
  next if paths_present.include?(path)

  if entry["ephemeral"]
    pruned << "#{path} (whole file, #{entry['class']})"
    next
  end

  failures << "#{path} is booked whole-file but the scan found nothing in it;\n" \
              "      the file moved, was renamed, or stopped stating the contract"
end

# ── Class A must cite the single source ──────────────────────────────────────
digest_cache = {}
booked.each do |(path, digest), entry|
  next unless CLASSES.fetch(entry["class"], { cites: false })[:cites]
  next unless present.include?([path, digest])

  cited_by = entry["citedBy"].to_s
  next if cited_by.empty?

  lines = (digest_cache[path] ||= digests_of(REPO_ROOT, path))
  unless lines.include?(cited_by)
    failures << "#{path} #{digest}: citedBy #{cited_by} matches no line in that file\n" \
                "      the citation was reworded or removed; the statement is now standing alone"
    next
  end

  # The citation must actually name the single source. A line that merely exists
  # is §4.4 shape 1 — text presence standing in for the fact — so the line's
  # content is read, not just its presence.
  citing = REPO_ROOT.join(path).read.each_line.find do |line|
    Digest::SHA256.hexdigest(line.strip)[0, 12] == cited_by
  end
  next if citing&.include?(canon)

  failures << "#{path} #{digest}: the citing line does not name #{canon}\n" \
              "      #{citing.to_s.strip[0, 90]}"
end

# ── The known limit, re-derived rather than asserted in a comment ────────────
mirror_roots = %w[.agents/skills/ .cursor/skills/]
unexplained = irregular.reject { |rel| mirror_roots.any? { |root| rel.start_with?(root) } }
unless unexplained.empty?
  failures << "#{unexplained.length} tracked path(s) are not regular files and are not skill mirrors, " \
              "so this scan does not read them: #{unexplained.sort.first(5).join(', ')}"
end

by_class = sites.group_by do |site|
  entry = booked[[site["path"], site["digest"]]] || whole_file[site["path"]]
  entry ? entry["class"] : "unclassified"
end

if failures.empty?
  puts "forward-only rollback contract: #{sites.length} site(s) in #{sites.map { |s| s['path'] }.uniq.length} file(s), " \
       "each classified"
  puts "  single source: #{canon}"
  CLASSES.each_key do |klass|
    found = by_class.fetch(klass, [])
    next if found.empty?

    puts format("  %-7s %2d site(s) / %2d file(s)  %s", klass, found.length,
                found.map { |s| s["path"] }.uniq.length, CLASSES.fetch(klass)[:summary])
  end
  cited = by_class.fetch("A", []).length
  puts "  #{cited} class-A statement(s) cite the single source, each by its own line"
  unless pruned.empty?
    puts "#{pruned.length} ephemeral entr(ies) no longer match and were pruned rather than failed:"
    pruned.sort.each { |line| puts "  - #{line}" }
    puts "  ephemeral is per entry with a reason, never a subtree (§4.4 shape 8)"
  end
  puts "scanned #{text.length} tracked text file(s), #{irregular.length} non-regular path(s) skipped"
  exit 0
end

failures.each { |line| warn "    #{line}" }
warn "    (scanned #{text.length} tracked text file(s))"
warn "    forward-only rollback contract: #{failures.length} failure(s)"
exit 1
