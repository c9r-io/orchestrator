#!/usr/bin/env ruby
# frozen_string_literal: true

# Constructs that bash 3.2 rejects, found in every tracked shell file.
#
# macOS ships bash 3.2 and nothing newer, and the GitHub macOS runner image
# ships no newer bash on PATH either. Every `#!/usr/bin/env bash` script this
# repository runs on a macOS job therefore runs under 3.2, and constructs that
# are unremarkable in bash 4+ are hard errors there.
#
# That is not hypothetical. `scripts/coverage-governance.sh` expanded an empty
# array under `set -u` on line 38, which bash 3.2 reports as `unbound variable`
# and bash 4.4+ expands to zero words. The `boundary-coverage` job died on that
# line on every run from the commit that introduced it until FR-135 — the gate
# was wired and never once reached the thing it gates.
#
# There is no way to check this on a Linux runner. `BASH_COMPAT=3.2` was
# measured against bash 5.3 for every class below and restores none of them, so
# the semantic half of this check has to run where a real bash 3.2 exists. This
# file is the static half; `scripts/qa/test-bash32-compat.sh` is the half that
# executes each class under the real interpreter and asserts it behaves as
# claimed. Neither is sufficient alone: a pattern scan cannot tell you the
# pattern is actually dangerous, and a fixture corpus cannot tell you the
# repository is free of it.
#
# The scanned set comes from `git ls-files`, not from a list. A roster guards
# exactly what existed when it was written, and the next script lands outside it
# in silence.

require "open3"
require "optparse"
require "pathname"

module Bash32Compat
  # The canonical rewrite. Enforcing one spelling is deliberate: the gate blanks
  # this exact string before looking for bare expansions, so a second spelling
  # would read as a violation. The alternative — parsing arbitrary `${x[@]+...}`
  # bodies with their nested braces — is the kind of brace counting that has
  # broken other gates in this repository.
  def self.safe_form(name)
    %(${#{name}[@]+"${#{name}[@]}"})
  end

  Finding = Struct.new(:file, :line, :rule, :detail, :fix, keyword_init: true)

  # A shell file's lines with comments and here-document bodies removed.
  #
  # Both removals matter. A comment describing a hazard is not a hazard, and a
  # here-document body is data to the enclosing script — this gate's own wrapper
  # writes hazardous fixtures that way, and so do several of the QA wrappers.
  # Comment detection tracks quoting rather than matching `#`, because `'#'` and
  # `"...#..."` are ordinary characters and a bare regex reads them as comments.
  def self.code_lines(text)
    result = []
    heredoc_terminator = nil

    text.lines.each_with_index do |raw, index|
      line = raw.chomp

      if heredoc_terminator
        heredoc_terminator = nil if line.strip == heredoc_terminator
        next
      end

      code = strip_comment(line)
      result << [index + 1, code]

      opener = code[/<<-?\s*(?!<)(["']?)([A-Za-z_][A-Za-z0-9_]*)\1/, 2]
      heredoc_terminator = opener if opener
    end

    result
  end

  # Returns the line with any trailing unquoted comment removed.
  def self.strip_comment(line)
    in_single = false
    in_double = false
    index = 0

    while index < line.length
      char = line[index]

      if char == "\\" && !in_single
        index += 2
        next
      end

      if char == "'" && !in_double
        in_single = !in_single
      elsif char == '"' && !in_single
        in_double = !in_double
      elsif char == "#" && !in_single && !in_double && (index.zero? || line[index - 1] =~ /\s/)
        return line[0...index]
      end

      index += 1
    end

    line
  end

  # Array names that can hold zero elements at some point in the file.
  #
  # Two shapes produce one: an explicit empty literal, and capture of "$@" in a
  # function that may be called with no arguments. `provider_isolation.sh` has
  # the second and would be missed by a rule that only looked for `=()`.
  def self.emptyable_arrays(lines)
    names = {}

    lines.each do |number, code|
      code.scan(/(?<![\w$])([A-Za-z_][A-Za-z0-9_]*)=\(\s*\)/) do |(name)|
        names[name] ||= number
      end
      code.scan(/(?<![\w$])([A-Za-z_][A-Za-z0-9_]*)=\("\$@"\)/) do |(name)|
        names[name] ||= number
      end
    end

    names
  end

  # `${#a[@]}` and `${!a[@]}` were measured against bash 3.2 and are both fine on
  # an empty array; only the value expansions `${a[@]}` and `${a[*]}` are not.
  # Flagging the safe two would have sent `probe-runner-lib.sh` through a rewrite
  # that fixes nothing.
  def self.empty_expansion_findings(path, lines, names)
    findings = []

    lines.each do |number, code|
      names.each_key do |name|
        scrubbed = code.gsub(safe_form(name), "")
        next unless scrubbed =~ /\$\{#{Regexp.escape(name)}\[[@*]\]\}/

        findings << Finding.new(
          file: path,
          line: number,
          rule: "empty-array-expansion",
          detail: "`#{name}` is assigned an empty value in this file; bash 3.2 reports " \
                  "`#{name}[@]: unbound variable` when an empty array is expanded under `set -u`",
          fix: "write #{safe_form(name)}"
        )
      end
    end

    findings
  end

  # Builtins and expansions that bash 3.2 does not have at all. Every entry was
  # executed against /bin/bash 3.2 and the recorded failure is what it produced.
  BUILTIN_RULES = [
    {
      rule: "associative-array",
      pattern: /(?<![\w-])(?:declare|local|typeset)\s+-[A-Za-z]*A[A-Za-z]*\s/,
      detail: "bash 3.2 has no associative arrays: `declare: -A: invalid option`",
      fix: "use a `case` lookup function or parallel indexed arrays"
    },
    {
      rule: "mapfile",
      pattern: /(?<![\w-])(?:mapfile|readarray)(?![\w-])/,
      detail: "bash 3.2 has no `mapfile`/`readarray`: `mapfile: command not found`",
      fix: "use `while IFS= read -r line; do arr+=(\"$line\"); done < <(...)`"
    },
    {
      rule: "case-conversion",
      pattern: /\$\{[A-Za-z_][A-Za-z0-9_]*(?:\[[^\]]*\])?(?:\^\^?|,,?)/,
      detail: "bash 3.2 has no `${x^^}`/`${x,,}`: `bad substitution`",
      fix: "use `tr '[:lower:]' '[:upper:]'`"
    },
    {
      rule: "nameref",
      pattern: /(?<![\w-])(?:declare|local|typeset)\s+-[A-Za-z]*n[A-Za-z]*\s/,
      detail: "bash 3.2 has no name references: `local: -n: invalid option`",
      fix: "pass the value, or use `eval` with an explicit guard"
    },
    {
      rule: "wait-n",
      pattern: /(?<![\w-])wait\s+-n(?![\w-])/,
      detail: "bash 3.2 has no `wait -n`: `wait: -n: invalid option`",
      fix: "wait on explicit PIDs"
    },
    {
      rule: "globstar",
      pattern: /(?<![\w-])shopt\s+(?:-[A-Za-z]+\s+)*globstar(?![\w-])/,
      detail: "bash 3.2 has no `globstar`: `shopt: globstar: invalid shell option name`",
      fix: "use `find` or `git ls-files`"
    }
  ].freeze

  def self.builtin_findings(path, lines)
    findings = []

    lines.each do |number, code|
      BUILTIN_RULES.each do |rule|
        next unless code =~ rule[:pattern]

        findings << Finding.new(
          file: path,
          line: number,
          rule: rule[:rule],
          detail: rule[:detail],
          fix: rule[:fix]
        )
      end
    end

    findings
  end

  def self.scan_file(repo_root, relative)
    text = File.read(repo_root.join(relative))
    lines = code_lines(text)
    names = emptyable_arrays(lines)

    empty_expansion_findings(relative, lines, names) + builtin_findings(relative, lines)
  end

  # Coverage is whatever git tracks, so a new script is scanned the day it is
  # added and no list has to be remembered.
  def self.shell_files(repo_root)
    output, status = Open3.capture2("git", "-C", repo_root.to_s, "ls-files", "-z", "*.sh")
    raise "git ls-files failed" unless status.success?

    output.split("\0").reject(&:empty?).sort
  end

  def self.run(repo_root)
    files = shell_files(repo_root)
    findings = files.flat_map { |relative| scan_file(repo_root, relative) }
    [files, findings]
  end
end

if $PROGRAM_NAME == __FILE__
  repo_root = Pathname.new(File.expand_path("../..", __dir__))
  list_only = false

  OptionParser.new do |opts|
    opts.banner = "usage: bash32-compat.rb [--list-files]"
    opts.on("--list-files", "print the scanned set and exit") { list_only = true }
    opts.on("--repo-root PATH", "scan a different checkout") { |value| repo_root = Pathname.new(value) }
  end.parse!(ARGV)

  if list_only
    puts Bash32Compat.shell_files(repo_root)
    exit 0
  end

  files, findings = Bash32Compat.run(repo_root)

  findings.each do |finding|
    warn "#{finding.file}:#{finding.line}: [#{finding.rule}] #{finding.detail}"
    warn "  fix: #{finding.fix}"
  end

  if findings.empty?
    puts "bash 3.2 compatibility: PASS (#{files.length} shell file(s) scanned, 0 finding(s))"
    exit 0
  end

  warn "bash 3.2 compatibility: FAIL (#{files.length} shell file(s) scanned, #{findings.length} finding(s))"
  exit 1
end
