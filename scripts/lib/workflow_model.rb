#!/usr/bin/env ruby
#
# A structural view of GitHub Actions workflows, for gates that need to know what
# a job actually executes rather than what its text contains.
#
# FR-127's wiring check asked `grep -F "$script" "$job_block"`. FR-134 recorded
# what that accepts: a step whose `run:` has been commented out with an
# explanation beside it, the script's name appearing in a `name:` field, and a
# mention inside a heredoc. All three read as "wired" and none of them runs.
# "Referenced by the job" and "executed by the job" are different claims, and
# only the second one is worth gating on.
#
# This library answers the second. It reports facts about steps — the shell text
# that will really run, the packages a job installs, the actions it uses — and
# leaves interpretation to the caller. Mapping `ripgrep` to `rg` is a claim about
# Debian, not about this repository, so it lives in the manifest where it can be
# reviewed, not here.
#
# Usage (the bash gates shell out to these):
#   workflow_model.rb jobs <workflow>
#   workflow_model.rb run-commands <workflow> <job>
#   workflow_model.rb installs <workflow> <job>
#   workflow_model.rb runs-on <workflow> <job>
#   workflow_model.rb step-names <workflow> <job>

require "yaml"
require "date"

module WorkflowModel
  module_function

  def load(path)
    # Psych parses GitHub's `on:` key as the boolean true (YAML 1.1). Nothing
    # here reads triggers through the parsed document, so it does not matter —
    # but callers that need triggers should know before they go looking.
    YAML.safe_load(File.read(path), aliases: true, permitted_classes: [Date, Time])
  end

  def jobs(path)
    (load(path)["jobs"] || {}).keys
  end

  def job(path, name)
    (load(path)["jobs"] || {})[name]
  end

  def steps(path, name)
    definition = job(path, name)
    return [] unless definition

    definition["steps"] || []
  end

  # A step is disabled when its condition is the literal false. Anything else —
  # an expression, a `success()`, a matrix reference — may evaluate either way
  # at run time, so it counts as executable. Erring toward "runs" here is the
  # safe direction: it can only make the gate stricter.
  def disabled?(step)
    condition = step["if"]
    return false if condition.nil?

    condition == false || condition.to_s.strip.casecmp("false").zero?
  end

  # The shell text an enabled step will really execute: heredoc bodies and shell
  # comments removed. What survives is what a shell would treat as commands.
  def run_commands(path, name)
    steps(path, name).reject { |step| disabled?(step) }
      .map { |step| step["run"] }.compact
      .map { |script| executable_shell(script) }
      .join("\n")
  end

  # Heredoc bodies are data, not commands. A job that writes a script name into
  # a file it never runs has not wired that script to anything, and the stub
  # installer in ci.yml is exactly this shape — it heredocs a whole shell script
  # into $RUNNER_TEMP.
  def executable_shell(script)
    kept = []
    terminator = nil
    script.to_s.each_line do |line|
      if terminator
        terminator = nil if line.strip == terminator
        next
      end
      stripped = strip_comment(line)
      if (match = stripped.match(/<<[-~]?\s*(['"]?)([A-Za-z_][A-Za-z0-9_]*)\1/))
        terminator = match[2]
      end
      kept << stripped
    end
    kept.join
  end

  # Removes a shell comment without being fooled by a `#` inside quotes. A `#`
  # only opens a comment at the start of a word, which is what the preceding
  # character tells us.
  def strip_comment(line)
    in_single = false
    in_double = false
    index = 0
    while index < line.length
      char = line[index]
      if char == "\\" && in_double
        index += 2
        next
      end
      in_single = !in_single if char == "'" && !in_double
      in_double = !in_double if char == "\"" && !in_single
      if char == "#" && !in_single && !in_double
        previous = index.zero? ? nil : line[index - 1]
        return "#{line[0...index].rstrip}\n" if previous.nil? || previous.match?(/\s/)
      end
      index += 1
    end
    line
  end

  # Does `command` appear in the job as something that executes? Matched on word
  # boundaries against the executable text, so a substring of a longer path does
  # not count.
  def executes?(path, name, command)
    run_commands(path, name).match?(/(?:^|[\s;&|(`"'])#{Regexp.escape(command)}(?:$|[\s;&|)`"'])/)
  end

  # What the job puts on PATH, as raw facts. Three sources, because a job can
  # provide a command three ways and a check that knows only about apt would
  # report every toolchain action as a missing dependency.
  def installs(path, name)
    found = []
    steps(path, name).reject { |step| disabled?(step) }.each do |step|
      if (script = step["run"])
        executable_shell(script).scan(/apt-get\s+(?:-[^\s]+\s+)*install\s+([^\n&|;]*)/) do |packages|
          packages.first.split.reject { |token| token.start_with?("-") }
            .each { |package| found << ["apt", package] }
        end
        executable_shell(script).scan(/brew\s+install\s+([^\n&|;]*)/) do |packages|
          packages.first.split.reject { |token| token.start_with?("-") }
            .each { |package| found << ["brew", package] }
        end
      end
      next unless (uses = step["uses"])

      action = uses.split("@").first
      found << ["action", action]
      tool = (step["with"] || {})["tool"]
      # taiki-e/install-action names its tool in `with:`, optionally version
      # pinned; the command is the part before the @.
      found << ["action-tool", tool.to_s.split("@").first] if tool
    end
    found.uniq
  end

  def runs_on(path, name)
    definition = job(path, name)
    return [] unless definition

    declared = definition["runs-on"]
    labels = Array(declared).flatten
    matrix = ((definition["strategy"] || {})["matrix"] || {})
    labels.flat_map do |label|
      match = label.to_s.match(/\$\{\{\s*matrix\.([A-Za-z0-9_]+)\s*\}\}/)
      next [label.to_s] unless match

      key = match[1]
      values = matrix[key]
      if values.is_a?(Array)
        values.map(&:to_s)
      elsif matrix["include"].is_a?(Array)
        matrix["include"].map { |entry| entry[key]&.to_s }.compact.uniq
      else
        [label.to_s]
      end
    end.uniq
  end

  # The `fetch-depth` the job's checkout asks for, or "1" when it asks for
  # nothing — actions/checkout fetches a single commit by default, so a job that
  # says nothing has no history at all. Returns "none" when the job does not
  # check out.
  def checkout_depth(path, name)
    step = steps(path, name).reject { |candidate| disabled?(candidate) }
      .find { |candidate| candidate["uses"].to_s.start_with?("actions/checkout") }
    return "none" unless step

    ((step["with"] || {})["fetch-depth"] || 1).to_s
  end

  def step_names(path, name)
    steps(path, name).map { |step| step["name"] }.compact
  end

  # Every workflow in a checkout, discovered rather than listed. A gate that
  # reasons about workflows and names them in an array guards exactly the ones
  # that existed when it was written; the next one lands outside it silently.
  def workflows(root)
    Dir.glob(File.join(root, ".github", "workflows", "*.{yml,yaml}")).sort
  end

  # Steps whose failure will not fail the job. `continue-on-error` is only off
  # when it says so literally — an expression may evaluate either way at run
  # time, so it counts as on. Same direction as `disabled?`: erring toward "this
  # step's failure is swallowed" can only make a caller stricter.
  def continue_on_error_steps(path, name)
    steps(path, name).select do |step|
      value = step["continue-on-error"]
      next false if value.nil?

      !(value == false || value.to_s.strip.casecmp("false").zero?)
    end
  end

  # The step ids a job reads as `steps.<id>.outcome`, walked out of the parsed
  # job rather than scanned out of the file. The distinction matters: the same
  # text inside a neighbouring job, a comment, or a `name:` is not this job
  # consuming this step's outcome, and a byte-level scan cannot tell the
  # difference. Both index forms GitHub accepts are matched, because
  # `steps['my-id'].outcome` is the form anyone writing an id with a dot in it
  # is pushed toward.
  OUTCOME_REFERENCE = /
    steps
    (?:
      \.\s*(?<dot>[A-Za-z_][A-Za-z0-9_-]*)
      |
      \s*\[\s*(?<quote>['"])(?<bracket>[^'"]+)\k<quote>\s*\]
    )
    \s*\.\s*outcome
  /x.freeze

  def outcome_references(path, name)
    found = []
    walk_strings(job(path, name)) do |text|
      text.scan(OUTCOME_REFERENCE) { found << (Regexp.last_match[:dot] || Regexp.last_match[:bracket]) }
    end
    found.uniq
  end

  # Every `scripts/**` executable a workflow job really runs, for a whole
  # checkout, as `path <TAB> workflow <TAB> job` records.
  #
  # This is the fact FR-147 needed and could not get: the enforcement manifest
  # is a declaration, and until something derives what CI *executes* there is
  # nothing to compare it against. Three shell gates had been running in ci.yml
  # for months while absent from the manifest, so every scanner that derives its
  # scope from that manifest — jq-status-observed.rb, fixture-target-drift.rb —
  # had never once read them. A hand count is what missed them: the third was
  # found only by a reconciliation that derived the invocations instead.
  #
  # Read off `run_commands`, so the same three things that are not execution
  # elsewhere in this file are not execution here either: a commented-out `run:`,
  # an `if: false` step, and a name inside a heredoc body.
  #
  # Bulk, one process for the whole checkout, for the reason `outcome_facts`
  # gives: a process per job would put seconds into a gate that runs on every
  # push. Facts, not a verdict — whether an undeclared path is a gap or a
  # deliberate exemption is the manifest's business, and the caller's.
  SCRIPT_TOKEN = %r{(?<![\w/.-])\.?/?(scripts/[\w./-]*\.(?:sh|rb))}.freeze

  def executed_scripts(root)
    records = []
    workflows(root).each do |workflow|
      relative = workflow.sub(%r{\A#{Regexp.escape(root)}/?}, "").sub(%r{\A\./}, "")
      jobs(workflow).each do |name|
        run_commands(workflow, name).scan(SCRIPT_TOKEN) do |match|
          records << [match.first, relative, name]
        end
      end
    end
    records.uniq
  end

  # A workflow's triggers, as the bare event names. Psych parses GitHub's `on:`
  # as the boolean true under YAML 1.1 — the caveat `load` already warns about —
  # so the key is looked up both ways rather than assumed.
  def triggers(path)
    document = load(path)
    map = document.key?(true) ? document[true] : document["on"]
    case map
    when Hash then map
    when Array then map.to_h { |event| [event, nil] }
    when String then { map => nil }
    else {}
    end
  end

  # Does this workflow run on ordinary development activity? A branch push or a
  # pull request means "on every change"; a tag-filtered push or a manual
  # dispatch does not. The distinction is what lets a release-only script be
  # exempted from the enforcement surface without that exemption becoming a
  # place to hide a governance gate: move the script into a job of a workflow
  # that answers true here and the exemption stops applying.
  def development_triggered?(path)
    map = triggers(path)
    return true if map.key?("pull_request") || map.key?("pull_request_target")

    push = map["push"]
    return false unless map.key?("push")
    # `push:` with no filter is every branch and every tag.
    return true if push.nil? || !push.is_a?(Hash)

    push.key?("branches") || push.key?("branches-ignore")
  end

  def walk_strings(node, &block)
    case node
    when Hash then node.each { |key, value| walk_strings(key, &block); walk_strings(value, &block) }
    when Array then node.each { |value| walk_strings(value, &block) }
    when String then block.call(node)
    end
  end

  # Every fact a caller needs to decide whether a job aggregates what it
  # swallows, for a whole checkout, in one process. Three record kinds, tab
  # separated:
  #
  #   coe   <workflow> <job> <id-or-empty> <step name>
  #   step  <workflow> <job> <id>
  #   ref   <workflow> <job> <id read as steps.<id>.outcome>
  #
  # Facts, not a verdict — the set arithmetic that turns these into "this step's
  # failure disappears" belongs to the gate, beside the reason it is a rule.
  # Bulk because the caller runs it once per fixture tree and there are two
  # dozen of those; a process per job would put minutes into the gate that
  # FR-140 is open about.
  def outcome_facts(root)
    records = []
    workflows(root).each do |workflow|
      relative = workflow.sub(%r{\A#{Regexp.escape(root)}/?}, "")
      jobs(workflow).each do |name|
        continue_on_error_steps(workflow, name).each do |step|
          records << ["coe", relative, name, step["id"].to_s, step["name"].to_s]
        end
        steps(workflow, name).each do |step|
          records << ["step", relative, name, step["id"].to_s] if step["id"]
        end
        outcome_references(workflow, name).each do |id|
          records << ["ref", relative, name, id]
        end
      end
    end
    records
  end
end

if $PROGRAM_NAME == __FILE__
  command = ARGV.shift
  case command
  when "jobs"
    puts WorkflowModel.jobs(ARGV[0])
  when "run-commands"
    puts WorkflowModel.run_commands(ARGV[0], ARGV[1])
  when "installs"
    WorkflowModel.installs(ARGV[0], ARGV[1]).each { |kind, value| puts "#{kind}\t#{value}" }
  when "runs-on"
    puts WorkflowModel.runs_on(ARGV[0], ARGV[1])
  when "step-names"
    puts WorkflowModel.step_names(ARGV[0], ARGV[1])
  when "checkout-depth"
    puts WorkflowModel.checkout_depth(ARGV[0], ARGV[1])
  when "executes"
    exit(WorkflowModel.executes?(ARGV[0], ARGV[1], ARGV[2]) ? 0 : 1)
  when "workflows"
    puts WorkflowModel.workflows(ARGV[0] || ".")
  when "continue-on-error-steps"
    # id first so a caller can split on tab; an empty first field is a step whose
    # failure is swallowed and which has no id to report it under.
    WorkflowModel.continue_on_error_steps(ARGV[0], ARGV[1]).each do |step|
      puts "#{step["id"]}\t#{step["name"]}"
    end
  when "outcome-references"
    puts WorkflowModel.outcome_references(ARGV[0], ARGV[1])
  when "outcome-facts"
    WorkflowModel.outcome_facts(ARGV[0] || ".").each { |record| puts record.join("\t") }
  when "executed-scripts"
    WorkflowModel.executed_scripts(ARGV[0] || ".").each { |record| puts record.join("\t") }
  when "development-triggered"
    exit(WorkflowModel.development_triggered?(ARGV[0]) ? 0 : 1)
  else
    warn "usage: workflow_model.rb {jobs|run-commands|installs|runs-on|step-names|checkout-depth|" \
         "executes|workflows|continue-on-error-steps|outcome-references|outcome-facts|" \
         "executed-scripts|development-triggered} ..."
    exit 2
  end
end
