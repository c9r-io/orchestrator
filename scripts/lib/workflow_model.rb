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

  def step_names(path, name)
    steps(path, name).map { |step| step["name"] }.compact
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
  when "executes"
    exit(WorkflowModel.executes?(ARGV[0], ARGV[1], ARGV[2]) ? 0 : 1)
  else
    warn "usage: workflow_model.rb {jobs|run-commands|installs|runs-on|step-names|executes} ..."
    exit 2
  end
end
