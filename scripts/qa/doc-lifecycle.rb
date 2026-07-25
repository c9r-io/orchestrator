#!/usr/bin/env ruby

# Enforces lifecycle metadata on every design doc and QA doc, and regenerates the
# reverse index at config/governance/doc-lifecycle-index.json.
#
# The problem FR-132 names: this repo closes every FR into a DD + QA pair, so the
# document surface grows monotonically and nothing ever records that a document
# stopped being true. FR-126 needed four manual audit rounds to discover that
# DD-101/102/103 described an execution seam that had been deleted. A reader had
# no way to ask "does this still hold?" except by reading the whole document
# against the code.
#
# The lifecycle field answers that question mechanically, and the index makes the
# supersession edge navigable from the superseded document — the direction
# docs/feature_request/README.md cannot go, because it maps FR to DD only.
#
# `lifecycle` is deliberately not called `status`. 71 design docs already carry a
# `**Status**:` header holding Approved/Implemented/Released, which is
# implementation maturity at authoring time, not document currency. The two axes
# are independent: DD-101 is `**Status**: Released` *and* superseded. Reusing the
# word would put two meanings on one key inside the same file.
#
# Usage:
#   doc-lifecycle.rb                  verify the repository against the index
#   doc-lifecycle.rb --emit-index     print the candidate index
#   doc-lifecycle.rb --emit-index --write   apply it locally, then read the diff

require "json"
require "optparse"
require "pathname"
require "yaml"

# The JSON normalisation is shared with the coordination and core-boundary
# ledgers so all three serialise identically. It lives in rust_source.rb because
# that is where the first ledger needed it; moving it now would edit two green
# gates for no behavioural gain. Only the serialiser is used here.
require_relative "../lib/rust_source"

DOC_ROOTS = ["docs/design_doc", "docs/qa"].freeze
LIFECYCLES = %w[active superseded].freeze
RELATED_FR = /\AFR-\d+(?:,\s*FR-\d+)*\z/.freeze
SCOPE = "every Markdown file under docs/design_doc and docs/qa, excluding files named " \
  "README.md and any path containing a component that begins with an underscore".freeze

options = {
  index: "config/governance/doc-lifecycle-index.json",
  emit_index: false,
  write: false
}
OptionParser.new do |parser|
  parser.on("--index PATH") { |value| options[:index] = value }
  parser.on("--emit-index") { options[:emit_index] = true }
  parser.on("--write") { options[:write] = true }
end.parse!

repo_root = Pathname.new(File.expand_path("../..", __dir__))
index_path = repo_root.join(options[:index])

# Coverage is derived from the filesystem, never from a list. A hand-maintained
# roster guards exactly what was known when it was written, and the next document
# lands outside it silently — which is how enumeration-as-coverage fails.
def governed_documents(repo_root)
  DOC_ROOTS.flat_map do |root|
    Dir[repo_root.join(root, "**", "*.md").to_s].map { |path| Pathname.new(path) }
  end.select do |path|
    next false unless path.file?
    relative = path.relative_path_from(repo_root).to_s
    next false if path.basename.to_s == "README.md"
    next false if relative.split("/").any? { |part| part.start_with?("_") }
    true
  end.sort_by { |path| path.relative_path_from(repo_root).to_s }
end

# Parsed with a real YAML parser, not matched with a regex. `rg -q 'lifecycle:'`
# is satisfied by the word appearing in a prose paragraph or a fenced example
# halfway down the document, and a hand-rolled `key: value` regex silently
# mis-reads the shapes already in use here — block sequences under
# `self_referential_safe_scenarios` and `#` comment lines both appear in
# docs/qa today. Returns nil when the file does not open with a frontmatter
# block, and :invalid when the block is present but not parseable YAML.
def parse_frontmatter(text)
  lines = text.lines
  return nil if lines.empty?
  return nil unless lines[0].chomp == "---"
  closing = nil
  (1...lines.length).each do |i|
    if lines[i].chomp == "---"
      closing = i
      break
    end
  end
  return nil if closing.nil?
  begin
    parsed = YAML.safe_load(lines[1...closing].join)
  rescue Psych::SyntaxError => error
    return { "fields" => {}, "invalid" => error.message }
  end
  return { "fields" => {}, "invalid" => "frontmatter is not a mapping" } unless parsed.is_a?(Hash)
  { "fields" => parsed, "invalid" => nil }
end

def document_records(repo_root, documents)
  records = {}
  documents.each do |path|
    relative = path.relative_path_from(repo_root).to_s
    records[relative] = parse_frontmatter(File.read(path))
  end
  records
end

def validate(repo_root, records)
  errors = []
  records.sort.each do |relative, parsed|
    if parsed.nil?
      errors << "#{relative}: no frontmatter block; every governed document must declare `lifecycle`"
      next
    end
    unless parsed["invalid"].nil?
      errors << "#{relative}: frontmatter is not valid YAML: #{parsed["invalid"]}"
      next
    end
    fields = parsed["fields"]
    lifecycle = fields["lifecycle"]
    if lifecycle.nil? || (lifecycle.is_a?(String) && lifecycle.empty?)
      errors << "#{relative}: frontmatter has no `lifecycle`"
    elsif !LIFECYCLES.include?(lifecycle)
      # Presence is not validity. A typo that still parses would otherwise sit in
      # the index describing a state no reader can act on.
      errors << "#{relative}: lifecycle #{lifecycle.inspect} is not one of #{LIFECYCLES.join(", ")}"
    end

    related = fields["related_fr"]
    unless related.nil?
      if !related.is_a?(String) || !RELATED_FR.match?(related)
        errors << "#{relative}: related_fr #{related.inspect} is not FR-<number> (comma-separated when several)"
      end
    end

    target = fields["superseded_by"]
    target = nil if target.is_a?(String) && target.empty?
    if lifecycle == "superseded"
      if target.nil?
        errors << "#{relative}: lifecycle is superseded but no `superseded_by` names the successor"
      elsif !target.is_a?(String)
        errors << "#{relative}: superseded_by #{target.inspect} is not a repository-relative path"
      elsif target == relative
        errors << "#{relative}: superseded_by points at the document itself"
      elsif !repo_root.join(target).file?
        # FR-131's link gate resolves relative Markdown links in document bodies.
        # A frontmatter scalar is not a Markdown link, so it never sees this one.
        errors << "#{relative}: superseded_by #{target.inspect} does not resolve to a file in the repository"
      end
    elsif !target.nil?
      errors << "#{relative}: superseded_by is set but lifecycle is #{lifecycle.inspect}, not superseded"
    end
  end
  errors.concat(cycle_errors(records))
  errors
end

# A supersession chain that loops leaves a reader following pointers forever, and
# every document in the loop claims to be replaced by one that is itself replaced.
def cycle_errors(records)
  edges = {}
  records.each do |relative, parsed|
    next if parsed.nil?
    fields = parsed["fields"]
    next unless fields["lifecycle"] == "superseded"
    target = fields["superseded_by"]
    edges[relative] = target if target.is_a?(String) && !target.empty?
  end
  errors = []
  edges.keys.sort.each do |start|
    seen = [start]
    node = edges[start]
    while !node.nil? && edges.key?(node)
      break if seen.include?(node)
      seen << node
      node = edges[node]
    end
    next if node.nil? || !seen.include?(node)
    errors << "superseded_by chain forms a cycle: #{(seen + [node]).join(" -> ")}"
  end
  errors.uniq
end

def build_index(repo_root, records)
  documents = {}
  by_fr = Hash.new { |hash, key| hash[key] = [] }
  supersedes = Hash.new { |hash, key| hash[key] = [] }

  records.sort.each do |relative, parsed|
    fields = parsed.nil? ? {} : parsed["fields"]
    entry = { "lifecycle" => fields["lifecycle"] }
    related = fields["related_fr"]
    if related.is_a?(String) && !related.empty?
      list = related.split(",").map(&:strip)
      entry["relatedFr"] = list
      list.each { |fr| by_fr[fr] << relative }
    end
    target = fields["superseded_by"]
    if target.is_a?(String) && !target.empty?
      entry["supersededBy"] = target
      supersedes[target] << relative
    end
    documents[relative] = entry
  end

  {
    "schemaVersion" => 1,
    "scope" => SCOPE,
    "counts" => {
      "documents" => documents.length,
      "active" => documents.count { |_, entry| entry["lifecycle"] == "active" },
      "superseded" => documents.count { |_, entry| entry["lifecycle"] == "superseded" },
      "withRelatedFr" => documents.count { |_, entry| entry.key?("relatedFr") }
    },
    "documents" => documents,
    # The two reverse directions the FR asked for and that
    # docs/feature_request/README.md cannot express.
    "byFeatureRequest" => by_fr.keys.sort.map { |fr| [fr, by_fr[fr].sort] }.to_h,
    "supersedes" => supersedes.keys.sort.map { |doc| [doc, supersedes[doc].sort] }.to_h
  }
end

def index_report(expected, actual)
  before = (expected["documents"] || {})
  after = (actual["documents"] || {})
  lines = []
  (after.keys - before.keys).sort.each do |doc|
    lines << "  + #{doc} is not in the index"
  end
  (before.keys - after.keys).sort.each do |doc|
    lines << "  - #{doc} is in the index but not on disk"
  end
  (before.keys & after.keys).sort.each do |doc|
    next if before[doc] == after[doc]
    lines << "  ~ #{doc} #{before[doc].inspect} -> #{after[doc].inspect}"
  end
  %w[byFeatureRequest supersedes counts].each do |section|
    next if expected[section] == actual[section]
    lines << "  ~ #{section} differs from the reviewed index"
  end
  lines
end

documents = governed_documents(repo_root)
records = document_records(repo_root, documents)
errors = validate(repo_root, records)

if options[:emit_index]
  if options[:write]
    # A regenerated index is a proposal for a human to read in a diff. In CI there
    # is no human, and an automatic rewrite would turn the review gate into
    # decoration.
    if ENV.key?("CI")
      warn "refusing --write under CI: a regenerated index must be reviewed by a human"
      warn "run --emit-index locally, read the diff, and commit the index with the change"
      exit 2
    end
    unless errors.empty?
      warn "refusing to write an index built from documents that fail validation:"
      errors.each { |error| warn "  #{error}" }
      exit 1
    end
    File.write(index_path, RustSource.ledger_json(build_index(repo_root, records)))
    warn "wrote #{options[:index]}; review the diff and commit it with the change that caused it"
    exit 0
  end
  unless errors.empty?
    warn "refusing to emit an index built from documents that fail validation:"
    errors.each { |error| warn "  #{error}" }
    exit 1
  end
  print RustSource.ledger_json(build_index(repo_root, records))
  exit 0
end

if options[:write]
  warn "--write requires --emit-index"
  exit 2
end

unless index_path.file?
  warn "doc lifecycle index not found at #{options[:index]}"
  warn "generate it with --emit-index --write and commit it"
  exit 1
end

expected = JSON.parse(File.read(index_path))

if expected["scope"] != SCOPE
  errors << "index scope prose does not match the scan this gate implements; " \
    "the index describes something the gate does not measure"
end

if errors.empty?
  # Exact equality in both directions, not a monotonic ratchet. A ratchet that
  # only fires on growth lets a deletion pass silently, leaving the index
  # asserting documents the repository no longer has — green while saying
  # something false.
  drift = index_report(expected, build_index(repo_root, records))
  unless drift.empty?
    errors << "doc lifecycle index differs from the repository:\n#{drift.join("\n")}"
  end
end

if errors.empty?
  actual = build_index(repo_root, records)
  puts "Doc lifecycle: PASS"
  puts "  #{actual["counts"]["documents"]} governed document(s): " \
    "#{actual["counts"]["active"]} active, #{actual["counts"]["superseded"]} superseded"
  puts "  #{actual["counts"]["withRelatedFr"]} carry related_fr across " \
    "#{actual["byFeatureRequest"].length} feature request(s)"
  exit 0
end

warn "Doc lifecycle: FAIL"
errors.each { |error| warn "  #{error}" }
warn "  regenerate with --emit-index, review the diff, and commit the index " \
  "together with the change that caused it"
exit 1
