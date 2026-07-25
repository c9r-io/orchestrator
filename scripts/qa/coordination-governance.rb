#!/usr/bin/env ruby

require "json"
require "digest"
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
    unless Array(step["store_inputs"]).empty?
      touches << {
        "step" => step_id,
        "kind" => "store_inputs",
        "bindings" => Array(step["store_inputs"]).map do |input|
          {
            "store" => input["store"],
            "key" => input["key"],
            "as_var" => input["as_var"]
          }
        end.sort_by { |input| [input["store"], input["key"], input["as_var"]] }
      }
    end
    unless Array(step["store_outputs"]).empty?
      touches << {
        "step" => step_id,
        "kind" => "store_outputs",
        "bindings" => Array(step["store_outputs"]).map do |output|
          {
            "store" => output["store"],
            "key" => output["key"],
            "from_var" => output["from_var"]
          }
        end.sort_by { |output| [output["store"], output["key"], output["from_var"]] }
      }
    end
    unless Array(step["outputs"]).empty?
      touches << {
        "step" => step_id,
        "kind" => "outputs",
        "names" => Array(step["outputs"]).sort
      }
    end
    if step["pipe_to"]
      touches << {
        "step" => step_id,
        "kind" => "pipe_to",
        "target" => step["pipe_to"]
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

def explicit_driver_id(document)
  driver = document.dig("spec", "driver")
  return nil unless driver.is_a?(Hash)

  provider = driver["provider"].to_s
  return nil if provider.empty?

  transport = driver["transport"].to_s
  transport = "cli" if transport.empty?
  "#{provider}/#{transport}"
end

def canonical_json(value)
  case value
  when Hash
    value.keys.sort.to_h { |key| [key, canonical_json(value.fetch(key))] }
  when Array
    value.map { |item| canonical_json(item) }
  else
    value
  end
end

def execution_classification(driver_id)
  case driver_id
  when "shell/cli"
    "shell-script"
  when "claude/cli", "codex/cli"
    "ai-provider"
  else
    "unclassified"
  end
end

def agent_manifest_fingerprint(document)
  governed = {
    "kind" => document["kind"],
    "metadata" => {"name" => document.dig("metadata", "name")},
    "spec" => document["spec"]
  }
  Digest::SHA256.hexdigest(JSON.generate(canonical_json(governed)))
end

# This ratchet evaluates documents as candidates for reviewed production roots.
# It is deliberately stricter than daemon Apply: historical command-only Agents
# remain accepted at runtime ingress, warn, and are persisted as shell/cli.
def production_execution_document_accepted?(document)
  case document["kind"]
  when "Agent"
    !explicit_driver_id(document).nil?
  when "RuntimePolicy"
    document.dig("spec", "runner", "executor") != "streaming"
  else
    true
  end
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
    "celInterpreter" => 0,
    "legacyRunnerSelection" => 0
  }
  files.each do |path|
    source = File.read(path)
    if path.extname == ".rs"
      source = source.sub(
        /^\s*#\[cfg\(test\)\]\s*\n\s*mod tests\s*\{.*\z/m,
        ""
      )
    end
    source.each_line do |line|
      counts["capturesOrJsonPath"] += 1 if line.match?(/captures|json_path/)
      counts["pipelineVariables"] += 1 if line.include?("PipelineVariables")
      counts["celInterpreter"] += 1 if line.match?(/cel_interpreter|cel-interpreter/)
      counts["legacyRunnerSelection"] += 1 if line.match?(
        /RunnerExecutorKind|ShellRunnerExecutor|StreamingAgentRunner|spawn_with_runner(?:_and_capture)?_session|prepare_legacy_claude_streaming_command/
      )
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
  Array(fixture["executionCases"]).each do |test_case|
    layer = test_case["evaluationLayer"]
    rationale = test_case["rationale"].to_s.strip
    if layer != "production-manifest-governance"
      fixture_errors << "#{test_case.fetch("name")}: execution case must declare " \
        "evaluationLayer=production-manifest-governance"
    end
    if rationale.empty?
      fixture_errors << "#{test_case.fetch("name")}: execution case must explain its layer"
    end

    document = test_case.fetch("document")
    accepted = production_execution_document_accepted?(document)
    next if accepted == test_case.fetch("expectedAccepted")

    fixture_errors << "#{test_case.fetch("name")}: expected accepted=" \
      "#{test_case.fetch("expectedAccepted")}, got #{accepted}"
  end
  command_only_case = Array(fixture["executionCases"]).find do |test_case|
    document = test_case["document"] || {}
    document["kind"] == "Agent" &&
      document.dig("spec", "command").to_s.strip != "" &&
      explicit_driver_id(document).nil?
  end
  expected_runtime_compatibility = {
    "accepted" => true,
    "warningCode" => "legacy_agent_command_deprecated",
    "persistedDriver" => "shell/cli"
  }
  if command_only_case.nil?
    fixture_errors << "execution cases must include a command-only production rejection"
  elsif command_only_case["runtimeCompatibility"] != expected_runtime_compatibility
    fixture_errors << "command-only production rejection must document runtime " \
      "acceptance, warning, and shell/cli promotion"
  end
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
agents = []
runtime_policies = []
Array(ledger["productionRoots"]).each do |root|
  Dir[repo_root.join(root, "**/*.{yaml,yml}").to_s].sort.each do |path|
    YAML.load_stream(File.read(path)).compact.each do |document|
      next unless document.is_a?(Hash)
      if document["kind"] == "Workflow"
        documents << {
          "file" => relative_path(repo_root, path),
          "name" => document.dig("metadata", "name"),
          "touches" => workflow_touches(document),
          "directStepCommands" => Array(document.dig("spec", "steps")).each_with_object([]) do |step, commands|
            commands << step["id"] unless step["command"].to_s.empty?
          end
        }
      elsif document["kind"] == "Agent"
        driver_id = explicit_driver_id(document)
        agents << {
          "file" => relative_path(repo_root, path),
          "name" => document.dig("metadata", "name"),
          "legacyCommandOnly" => !document.dig("spec", "command").to_s.empty? &&
            document.dig("spec", "driver").nil?,
          "driver" => document.dig("spec", "driver"),
          "driverId" => driver_id,
          "classification" => execution_classification(driver_id),
          "migrationTarget" => driver_id,
          "manifestFingerprint" => agent_manifest_fingerprint(document)
        }
      elsif document["kind"] == "RuntimePolicy"
        runtime_policies << {
          "file" => relative_path(repo_root, path),
          "name" => document.dig("metadata", "name"),
          "executor" => document.dig("spec", "runner", "executor")
        }
      end
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

capture_consumers = documents.flat_map do |document|
  document["touches"].map do |touch|
    next unless touch["kind"] == "capture" ||
      (touch["kind"] == "post_action" && touch.key?("json_path"))
    {"file" => document["file"], "workflow" => document["name"], "touch" => touch}
  end.compact
end
pipeline_consumer_kinds = %w[
  capture
  step_vars
  store_inputs
  store_outputs
  outputs
  pipe_to
].freeze
pipeline_consumers = documents.flat_map do |document|
  document["touches"].map do |touch|
    next unless pipeline_consumer_kinds.include?(touch["kind"])
    {"file" => document["file"], "workflow" => document["name"], "touch" => touch}
  end.compact
end
governance_cel_names = %w[
  active_ticket_count
  api_publishable
  is_last_cycle
  mark_done
  qa_file_path
  self_referential_safe
  self_referential_safe_scenarios
  endsWith
  size
  startsWith
  tools_called
  true
  false
  in
].freeze
cel_coordination_consumers = documents.flat_map do |document|
  document["touches"].map do |touch|
    next unless %w[prehook convergence].include?(touch["kind"])
    expression = touch["expression"].to_s.gsub(
      /"(?:\\.|[^"])*"|'(?:\\.|[^'])*'/,
      " "
    )
    identifiers = expression.scan(/[A-Za-z_][A-Za-z0-9_]*/).uniq
    unexpected = identifiers - governance_cel_names
    next if unexpected.empty?
    {
      "file" => document["file"],
      "workflow" => document["name"],
      "touch" => touch,
      "coordinationIdentifiers" => unexpected.sort
    }
  end.compact
end
legacy_command_agents = agents.select { |agent| agent["legacyCommandOnly"] }
driver_ids = %w[shell/cli claude/cli codex/cli]
driver_ids |= agents.map { |agent| agent["driverId"] }.compact
driver_counts = driver_ids.to_h do |driver_id|
  [driver_id, agents.count { |agent| agent["driverId"] == driver_id }]
end
direct_step_commands = documents.flat_map do |document|
  document["directStepCommands"].map do |step|
    {"file" => document["file"], "workflow" => document["name"], "step" => step}
  end
end
global_streaming_executors = runtime_policies.select do |policy|
  policy["executor"] == "streaming"
end

expected_inventory = ledger["consumerInventory"] || {}
{
  "capturesOrJsonPath" => capture_consumers,
  "pipelineVariables" => pipeline_consumers,
  "celCoordination" => cel_coordination_consumers
}.each do |channel, consumers|
  expected = expected_inventory.dig(channel, "productionConsumerCount")
  if expected.is_a?(Integer) && expected != consumers.length
    errors << "#{channel} production consumer count changed from #{expected} to #{consumers.length}"
  end
end
expected_legacy_agents = retirement.dig("shellRunnerExecutor", "productionLegacyAgentCount")
if expected_legacy_agents.is_a?(Integer) && expected_legacy_agents != legacy_command_agents.length
  errors << "legacy command-only Agent count changed from #{expected_legacy_agents} to #{legacy_command_agents.length}"
end
expected_agent_count = retirement.dig("shellRunnerExecutor", "productionAgentCount")
if expected_agent_count.is_a?(Integer) && expected_agent_count != agents.length
  errors << "production Agent count changed from #{expected_agent_count} to #{agents.length}"
end
expected_driver_counts = retirement.dig("shellRunnerExecutor", "productionDriverCounts")
if expected_driver_counts.is_a?(Hash) && expected_driver_counts != driver_counts
  errors << "production driver counts changed from #{expected_driver_counts.inspect} to #{driver_counts.inspect}"
end
expected_agents = retirement.dig("shellRunnerExecutor", "productionAgents")
actual_agents = agents.sort_by { |agent| [agent["file"], agent["name"]] }.map do |agent|
  agent.slice(
    "file",
    "name",
    "classification",
    "migrationTarget",
    "manifestFingerprint"
  )
end
if expected_agents.is_a?(Array) && expected_agents != actual_agents
  errors << "production Agent execution inventory differs from the reviewed ledger"
end
expected_direct_commands =
  retirement.dig("shellRunnerExecutor", "productionDirectStepCommandCount")
if expected_direct_commands.is_a?(Integer) &&
    expected_direct_commands != direct_step_commands.length
  errors << "production direct Step command count changed from #{expected_direct_commands} to #{direct_step_commands.length}"
end
expected_global_streaming =
  retirement.dig("shellRunnerExecutor", "productionGlobalStreamingExecutorCount")
if expected_global_streaming.is_a?(Integer) &&
    expected_global_streaming != global_streaming_executors.length
  errors << "global streaming executor count changed from #{expected_global_streaming} to #{global_streaming_executors.length}"
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
    "celInterpreter",
    "legacyRunnerSelection"
  ),
  "preservedChannels" => actual_channels,
  "typedStateDecision" => ledger.dig("decision", "typedState"),
  "productionConsumers" => {
    "capturesOrJsonPath" => capture_consumers,
    "pipelineVariables" => pipeline_consumers,
    "celCoordination" => cel_coordination_consumers
  },
  "executionInventory" => {
    "agentDocuments" => agents.length,
    "agents" => agents.sort_by { |agent| [agent["file"], agent["name"]] }.map do |agent|
      agent.merge(
        "workflows" => documents
          .select { |document| document["file"] == agent["file"] }
          .map { |document| document["name"] }
          .sort
      )
    end,
    "legacyCommandOnlyAgents" => legacy_command_agents,
    "driverCounts" => driver_counts,
    "directStepCommands" => direct_step_commands,
    "globalStreamingExecutors" => global_streaming_executors
  },
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
