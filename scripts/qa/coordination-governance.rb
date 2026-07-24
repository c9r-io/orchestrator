#!/usr/bin/env ruby

require "json"
require "optparse"
require "pathname"
require "yaml"

KNOWN_BUILTINS = %w[
  init_once
  ticket_scan
  self_test
  self_restart
  item_select
  loop_guard
].freeze
CLASSIFICATIONS = %w[tool-migratable governance-only hybrid].freeze
COMPLETE_STATUSES = %w[migrated classified].freeze

options = {
  ledger: "config/governance/coordination-collapse-ledger.json",
  output: nil,
  require_complete: false,
  test_fixtures: false
}
OptionParser.new do |parser|
  parser.on("--ledger PATH") { |value| options[:ledger] = value }
  parser.on("--output PATH") { |value| options[:output] = value }
  parser.on("--require-complete") { options[:require_complete] = true }
  parser.on("--test-fixtures") { options[:test_fixtures] = true }
end.parse!

repo_root = Pathname.new(File.expand_path("../..", __dir__))
ledger_path = repo_root.join(options[:ledger])
ledger = JSON.parse(File.read(ledger_path))
errors = []

def relative_path(root, path)
  Pathname.new(path).relative_path_from(root).to_s
end

def workflow_touches(document)
  touches = []
  Array(document.dig("spec", "steps")).each do |step|
    step_id = step["id"]
    behavior = step["behavior"] || {}
    Array(behavior["captures"]).each do |capture|
      touch = {
        "step" => step_id,
        "kind" => "capture",
        "name" => capture["var"]
      }
      touch["json_path"] = capture["json_path"] if capture.key?("json_path")
      touches << touch
    end
    Array(behavior["post_actions"]).each do |action|
      touch = {
        "step" => step_id,
        "kind" => "post_action",
        "name" => action["type"]
      }
      touch["from_var"] = action["from_var"] if action.key?("from_var")
      touch["json_path"] = action["json_path"] if action.key?("json_path")
      touches << touch
    end
    if step["prehook"]
      touches << {
        "step" => step_id,
        "kind" => "prehook",
        "engine" => step.dig("prehook", "engine"),
        "expression" => step.dig("prehook", "when")
      }
    end
    builtin = step["builtin"]
    builtin ||= step["type"] if KNOWN_BUILTINS.include?(step["type"])
    if builtin
      touches << {
        "step" => step_id,
        "kind" => "builtin",
        "name" => builtin
      }
    end
    unless (step["step_vars"] || {}).empty?
      touches << {
        "step" => step_id,
        "kind" => "step_vars",
        "names" => step["step_vars"].keys.sort
      }
    end
  end
  Array(document.dig("spec", "loop", "convergence_expr")).each_with_index do |expression, index|
    touches << {
      "step" => "$loop[#{index}]",
      "kind" => "convergence",
      "engine" => expression["engine"],
      "expression" => expression["when"]
    }
  end
  touches
end

def rust_source_files(repo_root)
  roots = [repo_root.join("core/src")]
  roots.concat(Dir[repo_root.join("crates/*/src").to_s].map { |path| Pathname.new(path) })
  files = roots.flat_map do |root|
    Dir[root.join("**/*").to_s].each_with_object([]) do |path, files|
      pathname = Pathname.new(path)
      next unless pathname.file?
      next unless pathname.extname == ".rs"
      relative = pathname.relative_path_from(repo_root).to_s
      next if relative.split("/").include?("tests")
      next if pathname.basename.to_s.match?(/test.*\.rs\z/)
      files << pathname
    end
  end
  manifests = [repo_root.join("core/Cargo.toml")]
  manifests.concat(Dir[repo_root.join("crates/*/Cargo.toml").to_s].map { |path| Pathname.new(path) })
  files + manifests.select(&:file?)
end

def source_counts(files)
  counts = {
    "capturesOrJsonPath" => 0,
    "pipelineVariables" => 0,
    "celInterpreter" => 0
  }
  files.each do |path|
    File.foreach(path) do |line|
      counts["capturesOrJsonPath"] += 1 if line.match?(/captures|json_path/)
      counts["pipelineVariables"] += 1 if line.include?("PipelineVariables")
      counts["celInterpreter"] += 1 if line.match?(/cel_interpreter|cel-interpreter/)
    end
  end
  counts
end

if options[:test_fixtures]
  fixture_path = repo_root.join(
    "scripts/qa/fixtures/coordination-governance-cases.json"
  )
  fixture = JSON.parse(File.read(fixture_path))
  fixture_errors = []
  Array(fixture["cases"]).each do |test_case|
    touches = workflow_touches(test_case.fetch("workflow"))
    unexpected = touches - Array(test_case["approvedTouches"])
    accepted = unexpected.empty?
    next if accepted == test_case.fetch("expectedAccepted")

    fixture_errors << "#{test_case.fetch("name")}: expected accepted=" \
      "#{test_case.fetch("expectedAccepted")}, got #{accepted}"
  end
  unless fixture_errors.empty?
    fixture_errors.each { |error| warn "  - #{error}" }
    exit 1
  end
end

documents = []
Array(ledger["productionRoots"]).each do |root|
  Dir[repo_root.join(root, "**/*.{yaml,yml}").to_s].sort.each do |path|
    YAML.load_stream(File.read(path)).compact.each do |document|
      next unless document.is_a?(Hash) && document["kind"] == "Workflow"
      documents << {
        "file" => relative_path(repo_root, path),
        "name" => document.dig("metadata", "name"),
        "touches" => workflow_touches(document)
      }
    end
  rescue Psych::SyntaxError => error
    errors << "#{relative_path(repo_root, path)} is not valid YAML: #{error.message}"
  end
end

ledger_workflows = Array(ledger["workflows"])
ledger_keys = ledger_workflows.map { |workflow| [workflow["file"], workflow["name"]] }.sort
document_keys = documents.map { |workflow| [workflow["file"], workflow["name"]] }.sort
errors << "production Workflow inventory differs from the reviewed ledger" unless ledger_keys == document_keys

ledger_workflows.each do |workflow|
  classification = workflow["classification"]
  unless CLASSIFICATIONS.include?(classification)
    errors << "#{workflow["name"]}: invalid classification #{classification.inspect}"
  end
  if options[:require_complete] && !COMPLETE_STATUSES.include?(workflow["status"])
    errors << "#{workflow["name"]}: migration status is #{workflow["status"].inspect}, expected migrated/classified"
  end
  if options[:require_complete] && classification != "governance-only"
    evidence = workflow["evidence"]
    if evidence.nil? || evidence.empty? || !repo_root.join(evidence).file?
      errors << "#{workflow["name"]}: completed migration lacks a repository evidence artifact"
    end
  end
  current = documents.find do |document|
    document["file"] == workflow["file"] && document["name"] == workflow["name"]
  end
  next unless current
  approved = Array(workflow["approvedTouches"])
  unexpected = current["touches"] - approved
  missing = approved - current["touches"]
  unexpected.each do |touch|
    errors << "#{workflow["name"]}: unapproved coordination/governance touch #{JSON.generate(touch)}"
  end
  missing.each do |touch|
    errors << "#{workflow["name"]}: ledger touch is stale and must be reviewed #{JSON.generate(touch)}"
  end
end

expected_channels = {
  "goal" => "user-intent",
  "last_sandbox_denied" => "safety-signal",
  "sandbox_denied_count" => "safety-signal",
  "last_sandbox_denial_reason" => "safety-signal"
}
actual_channels = Array(ledger["preservedChannels"]).to_h do |channel|
  [channel["name"], channel["classification"]]
end
errors << "preserved residual channels differ from DD-130" unless actual_channels == expected_channels
unless ledger.dig("decision", "typedState") == "closed-not-deferred"
  errors << "typed-state decision must remain closed-not-deferred"
end

source_counts = source_counts(rust_source_files(repo_root))
source_baseline = ledger["sourceBaseline"] || {}
source_counts.each do |name, count|
  baseline = source_baseline[name]
  if !baseline.is_a?(Integer)
    errors << "source baseline #{name} is missing"
  elsif count > baseline
    errors << "source touch #{name} increased from #{baseline} to #{count}"
  end
end

retirement = ledger["retirement"] || {}
%w[freeze deprecate remove shellRunnerExecutor].each do |stage|
  errors << "retirement policy #{stage} is missing" unless retirement.key?(stage)
end

report = {
  "schemaVersion" => 1,
  "workflowCount" => documents.length,
  "classifications" => ledger_workflows.group_by { |workflow| workflow["classification"] }
    .transform_values(&:length),
  "statuses" => ledger_workflows.group_by { |workflow| workflow["status"] }
    .transform_values(&:length),
  "sourceTouches" => source_counts,
  "sourceBaseline" => source_baseline.slice(
    "capturesOrJsonPath",
    "pipelineVariables",
    "celInterpreter"
  ),
  "preservedChannels" => actual_channels,
  "typedStateDecision" => ledger.dig("decision", "typedState"),
  "errors" => errors
}

if options[:output]
  output = repo_root.join(options[:output])
  output.dirname.mkpath
  File.write(output, JSON.pretty_generate(report) + "\n")
end

if errors.empty?
  puts "Coordination governance: PASS"
  puts JSON.pretty_generate(report)
  exit 0
end

warn "Coordination governance: FAIL"
errors.each { |error| warn "  - #{error}" }
exit 1
