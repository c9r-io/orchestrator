#!/usr/bin/env ruby

require "json"
require "digest"
require "optparse"
require "pathname"
require "yaml"
require_relative "../lib/rust_source"
require_relative "../lib/ci_env"

# The Rust source scan and the ledger serialisation are shared with
# scripts/qa/core-boundary.rb. Both ledgers must count the same tree the same
# way, so the scanner is one file rather than two lookalikes (DD-142).
include RustSource

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
  test_fixtures: false,
  emit_inventory: false,
  emit_baseline: false,
  emit_consumers: false,
  write: false
}
OptionParser.new do |parser|
  parser.on("--ledger PATH") { |value| options[:ledger] = value }
  parser.on("--output PATH") { |value| options[:output] = value }
  parser.on("--require-complete") { options[:require_complete] = true }
  parser.on("--test-fixtures") { options[:test_fixtures] = true }
  parser.on("--emit-inventory") { options[:emit_inventory] = true }
  parser.on("--emit-baseline") { options[:emit_baseline] = true }
  parser.on("--emit-consumers") { options[:emit_consumers] = true }
  parser.on("--write") { options[:write] = true }
end.parse!

repo_root = Pathname.new(File.expand_path("../..", __dir__))
ledger_path = repo_root.join(options[:ledger])
ledger = JSON.parse(File.read(ledger_path))
errors = []

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

INVENTORY_FIELDS = %w[
  file
  name
  classification
  migrationTarget
  manifestFingerprint
].freeze

# The single definition of the reviewed production Agent inventory. Both the
# ledger comparison and --emit-inventory call this, so a regenerated candidate
# cannot differ in ordering or field selection from what the gate compares.
def production_agent_inventory(agents)
  agents
    .sort_by { |agent| [agent["file"], agent["name"]] }
    .map { |agent| agent.slice(*INVENTORY_FIELDS) }
end

# The ledger records a fingerprint, never a spec, so the reviewed spec exists
# nowhere in it and a fingerprint mismatch cannot by itself say what changed.
# HEAD is the reviewed state precisely because the ledger and the spec it
# describes must move in one commit (DD-140). That rule is what makes this diff
# well defined; when it is broken, the report below says so instead of guessing.
def head_agent_specs(repo_root, file)
  blob = IO.popen(
    ["git", "-C", repo_root.to_s, "show", "HEAD:#{file}"],
    err: File::NULL,
    &:read
  )
  return nil unless $?.success?

  YAML.load_stream(blob).compact.each_with_object({}) do |document, specs|
    next unless document.is_a?(Hash) && document["kind"] == "Agent"
    specs[document.dig("metadata", "name")] = document["spec"]
  end
rescue Psych::SyntaxError, Errno::ENOENT, SystemCallError
  nil
end

def spec_change_description(repo_root, head_cache, file, name, current_spec)
  head_cache[file] = head_agent_specs(repo_root, file) unless head_cache.key?(file)
  reviewed = head_cache[file]
  return "manifestFingerprint changed; the HEAD copy of #{file} is unreadable, " \
    "so the changed spec keys cannot be derived" if reviewed.nil?

  before = reviewed[name]
  return "manifestFingerprint changed; #{name} is absent from the HEAD copy of #{file}" if before.nil?

  if before == current_spec
    return "manifestFingerprint changed but the spec already matches HEAD, so the " \
      "spec was committed without its ledger update; they must land in one commit"
  end

  before_keys = before.is_a?(Hash) ? before.keys : []
  after_keys = current_spec.is_a?(Hash) ? current_spec.keys : []
  changed = (before_keys | after_keys).sort.reject do |key|
    (before.is_a?(Hash) ? before[key] : nil) == (current_spec.is_a?(Hash) ? current_spec[key] : nil)
  end
  "manifestFingerprint changed in spec key(s): #{changed.join(", ")}"
end

def inventory_mismatch_report(repo_root, expected, actual, specs)
  identity = ->(entry) { [entry["file"], entry["name"]] }
  expected_by = expected.to_h { |entry| [identity.call(entry), entry] }
  actual_by = actual.to_h { |entry| [identity.call(entry), entry] }
  head_cache = {}
  lines = []

  (actual_by.keys - expected_by.keys).sort.each do |file, name|
    lines << "  + #{file}##{name} exists in the repository and not in the ledger"
  end
  (expected_by.keys - actual_by.keys).sort.each do |file, name|
    lines << "  - #{file}##{name} exists in the ledger and not in the repository"
  end
  (expected_by.keys & actual_by.keys).sort.each do |key|
    before = expected_by[key]
    after = actual_by[key]
    next if before == after

    file, name = key
    changes = %w[classification migrationTarget].map do |field|
      next if before[field] == after[field]
      "#{field} #{before[field].inspect} -> #{after[field].inspect}"
    end.compact
    if before["manifestFingerprint"] != after["manifestFingerprint"]
      changes << spec_change_description(repo_root, head_cache, file, name, specs[key])
    end
    lines << "  ~ #{file}##{name}: #{changes.join("; ")}"
  end

  lines << "  regenerate with --emit-inventory, review the diff, and commit the " \
    "ledger together with the change that caused it"
  lines
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

def source_counts(files)
  counts = {
    "capturesOrJsonPath" => 0,
    "pipelineVariables" => 0,
    "celInterpreter" => 0,
    "legacyRunnerSelection" => 0
  }
  files.each do |path|
    # Masked, so a coordinate is counted where it is *code*. Four of these
    # numbers were part prose: the validator that rejects `behavior.captures`
    # names it in the rejection message, so the code deleting the surface
    # counted as code using it — DD-148's recorded shape, measured here at 23
    # lines of which 11 were real. See RustSource.masked_scannable_source.
    source = masked_scannable_source(path)

    # Occurrences, not lines. A line-count ratchet cannot see a second reference
    # added to a line that already has one, which is the cheapest way to grow a
    # coupling past it; DD-140 recorded all four of these as line-count regexes
    # and this is the half of that limit which masking does not address. The
    # `rusqlite` ruler next door has always counted occurrences, so after this
    # the two ledgers count the same tree the same way (DD-142's premise).
    #
    # `output_json_path` is the session/step structured-output spill path — a
    # live artifact location with no relation to the retired JSONPath
    # extraction surface this coordinate exists to count. An unanchored
    # `json_path` matched it anyway, and it was the majority of the number:
    # 32 of 55 lines when FR-159 tripped the ratchet by referencing the field
    # twice more. The lookbehind stays after masking: `output_json_path` is an
    # identifier, so masking never touched it and removing the anchor here
    # would put that field straight back into the count.
    counts["capturesOrJsonPath"] += source.scan(/captures|(?<!output_)json_path/).length
    counts["pipelineVariables"] += source.scan(/PipelineVariables/).length
    counts["celInterpreter"] += source.scan(/cel_interpreter|cel-interpreter/).length
    counts["legacyRunnerSelection"] += source.scan(
      /RunnerExecutorKind|ShellRunnerExecutor|StreamingAgentRunner|spawn_with_runner(?:_and_capture)?_session|prepare_legacy_claude_streaming_command/
    ).length
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
agent_specs = {}
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
        agent_specs[[relative_path(repo_root, path), document.dig("metadata", "name")]] =
          document["spec"]
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

actual_agents = production_agent_inventory(agents)

# Consumer inventories are computed here, above the emit block, because
# --emit-consumers regenerates the counts the gate compares against and so has
# to see them before the comparison turns a stale count into an error.
capture_consumers = documents.flat_map do |document|
  document["touches"].map do |touch|
    next unless touch["kind"] == "capture" ||
      (touch["kind"] == "post_action" && touch.key?("json_path"))
    {"file" => document["file"], "workflow" => document["name"], "touch" => touch}
  end.compact
end
# The step-level constructs that write an author-chosen value into the generic
# pipeline-variable map. All are rejected at apply with
# [legacy_pipeline_variables_removed] (FR-156), and this list must stay equal to
# what that rejection covers — a kind counted here but not rejected would let
# the ledger claim a surface is closed while it is only unread.
#
# `capture` was on this list and is not any more: behavior.captures belongs to
# the capturesOrJsonPath coordinate, which already carries it, and counting it
# in both meant one construct showing up as two consumers.
#
# `outputs` and `pipe_to` are gone for a different reason: they were never
# wired. Neither is a field of WorkflowStepSpec, and every conversion path set
# the WorkflowStepConfig fields to empty unconditionally, so no manifest could
# populate them and nothing read them. Counting them as "production consumers"
# counted something that could not have one. FR-156 deleted the fields; an
# author who writes `outputs:` now gets the ordinary unknown-field warning.
pipeline_consumer_kinds = %w[
  step_vars
  store_inputs
  store_outputs
].freeze
pipeline_consumers = documents.flat_map do |document|
  document["touches"].map do |touch|
    # The store_put post-action is the sixth. It reads a pipeline variable by
    # name and writes it to a store, and no coordinate counted it: capture
    # consumers take a post_action only when it carries json_path, and this list
    # matches on kind, which for every post-action is "post_action".
    is_store_put = touch["kind"] == "post_action" && touch["name"] == "store_put"
    next unless pipeline_consumer_kinds.include?(touch["kind"]) || is_store_put
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

if options[:emit_inventory] || options[:emit_baseline] || options[:emit_consumers]
  unless errors.empty?
    warn "refusing to emit a candidate from a repository that does not parse:"
    errors.each { |error| warn "  - #{error}" }
    exit 1
  end

  candidate = {}
  candidate["productionAgents"] = actual_agents if options[:emit_inventory]
  if options[:emit_baseline]
    baseline = (ledger["sourceBaseline"] || {}).dup
    source_counts(rust_source_files(repo_root)).each { |name, count| baseline[name] = count }
    candidate["sourceBaseline"] = baseline
  end
  if options[:emit_consumers]
    # Only the counts are regenerated. Everything else in a consumerInventory
    # entry -- state, scope, retainedCarrier, the code-level blockers -- is a
    # reviewed judgement about what the count means, and a tool that rewrote it
    # would be deciding rather than measuring. FR-156 added this because its own
    # acceptance criterion asked for a count "produced by the regeneration tool"
    # and no such emitter existed: --emit-inventory covers production Agents
    # only, so the number the gate compares was the one number in the ledger a
    # human had to type.
    inventory = (ledger["consumerInventory"] || {}).dup
    {
      "capturesOrJsonPath" => capture_consumers,
      "pipelineVariables" => pipeline_consumers,
      "celCoordination" => cel_coordination_consumers
    }.each do |channel, consumers|
      entry = (inventory[channel] || {}).dup
      entry["productionConsumerCount"] = consumers.length
      inventory[channel] = entry
    end
    candidate["consumerInventory"] = inventory
  end

  if options[:write]
    # A regenerated candidate is a proposal for a human to review in a diff. In
    # CI there is no human, and an automatic ledger rewrite would turn the
    # review gate into decoration.
    CiEnv.refuse_unattended_write!(
      "ledger",
      "run the emit modes locally, read the diff, and commit the ledger with the spec change"
    )
    updated = ledger
    updated["retirement"]["shellRunnerExecutor"]["productionAgents"] = candidate["productionAgents"] if candidate.key?("productionAgents")
    updated["sourceBaseline"] = candidate["sourceBaseline"] if candidate.key?("sourceBaseline")
    updated["consumerInventory"] = candidate["consumerInventory"] if candidate.key?("consumerInventory")
    File.write(ledger_path, ledger_json(updated))
    warn "wrote #{options[:ledger]}; review the diff and commit it with the change that caused it"
    exit 0
  end

  # A single flag emits that section bare so it can be diffed against the
  # ledger slice directly; both flags emit a keyed object.
  payload = candidate.length == 1 ? candidate.values.first : candidate
  puts JSON.pretty_generate(payload)
  exit 0
end

if options[:write]
  warn "--write requires --emit-inventory, --emit-baseline and/or --emit-consumers"
  exit 2
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
  # Exact, not monotonic. A count that drops below its baseline leaves the
  # ledger asserting debt the repository no longer carries, and the gate stays
  # green while saying something false. FR-128 found capturesOrJsonPath sitting
  # at 54 against a reviewed 55 for exactly that reason. --emit-baseline is the
  # recovery, so tightening costs a regeneration rather than an argument.
  if !baseline.is_a?(Integer)
    errors << "source baseline #{name} is missing"
  elsif count != baseline
    direction = count > baseline ? "increased" : "decreased"
    errors << "source touch #{name} #{direction} from #{baseline} to #{count}; " \
      "regenerate with --emit-baseline and review the diff"
  end
end

retirement = ledger["retirement"] || {}
%w[freeze deprecate remove shellRunnerExecutor].each do |stage|
  errors << "retirement policy #{stage} is missing" unless retirement.key?(stage)
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
if expected_agents.is_a?(Array) && expected_agents != actual_agents
  errors << "production Agent execution inventory differs from the reviewed ledger"
  inventory_mismatch_report(repo_root, expected_agents, actual_agents, agent_specs).each do |line|
    errors << line
  end
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
