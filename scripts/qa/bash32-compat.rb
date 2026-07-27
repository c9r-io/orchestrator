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

require_relative "../lib/shell_lexer"

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

  # A shell file's lines with comments, single-quoted regions and here-document
  # bodies removed.
  #
  # All three removals matter. A comment describing a hazard is not a hazard, a
  # single-quoted region is inert to the shell, and a here-document body is data
  # to the enclosing script — this gate's own wrapper writes hazardous fixtures
  # that way, and so do several of the QA wrappers.
  #
  # The lexing lives in `scripts/lib/shell_lexer.rb` because it has to carry
  # state across lines. Deciding per line is what produced FR-138: a `<< WORD`
  # lookalike inside a region opened earlier read as a here-document, and the
  # rest of the file left the scan with no diagnostic at all.
  def self.code_lines(text)
    ShellLexer.code_lines(text).first
  end

  # A file that ends inside a here-document was never fully scanned. Whether the
  # opener is a real unterminated body or a lookalike inside quoting the lexer
  # got wrong, the honest report is the same: everything after this line was
  # dropped. This is the backstop that does not depend on the lexer being right.
  def self.unclosed_heredoc_finding(path, state)
    return nil unless state.in_heredoc?

    Finding.new(
      file: path,
      line: state.heredoc_line,
      rule: "unclosed-heredoc",
      detail: "this file ends while still inside a here-document opened here with terminator " \
              "`#{state.heredoc}`; every line after it was dropped from the scan, so the rest " \
              "of the file is unchecked",
      fix: "close the here-document with a line reading exactly `#{state.heredoc}`, or — if this " \
           "is not a here-document at all — check what quoting the `<<` sits inside"
    )
  end

  # Every value expansion of an array that is not written in the canonical
  # guarded form.
  #
  # This used to fire only for arrays the scan could see being emptied in the
  # same file. That inference was wrong in both directions and only one of them
  # was visible: it over-reported where an earlier guard had already proved the
  # array non-empty (recorded in DD-146), and it silently missed arrays emptied
  # in a `source`d library and expanded in the caller (FR-138 defect B). Both
  # come from the same rule, so FR-138 removed the rule rather than extending it.
  # Matching beats inferring here: there is no inference surface left to route
  # around, and the guarded form costs nothing where it is not needed.
  #
  # `${#a[@]}` and `${!a[@]}` were measured against bash 3.2 and are both fine on
  # an empty array; only the value expansions `${a[@]}` and `${a[*]}` are not.
  # Flagging the safe two would have sent `probe-runner-lib.sh` through a rewrite
  # that fixes nothing.
  def self.empty_expansion_findings(path, lines)
    findings = []

    lines.each do |number, code|
      # Blank the canonical form first, so its own inner `${a[@]}` does not read
      # as a bare expansion.
      scrubbed = code.gsub(/\$\{([A-Za-z_][A-Za-z0-9_]*)\[@\]\+"\$\{\1\[@\]\}"\}/, "")

      scrubbed.scan(/\$\{([A-Za-z_][A-Za-z0-9_]*)\[[@*]\]\}/) do |(name)|
        findings << Finding.new(
          file: path,
          line: number,
          rule: "empty-array-expansion",
          detail: "bash 3.2 reports `#{name}[@]: unbound variable` when an empty array is " \
                  "expanded under `set -u`, and whether `#{name}` can be empty here is not a " \
                  "question this scan answers",
          fix: "write #{safe_form(name)}"
        )
      end
    end

    findings
  end

  # A builtin only matters where a builtin can run. Without this, the rules
  # below match the word wherever it appears — in a path like
  # `$WORK/hazard/mapfile.sh`, in a space-separated list of rule names, in any
  # string that happens to contain it. Those are mentions, and the subject here
  # is invocation. Expansion rules do not use it, because an expansion is not a
  # command.
  #
  # `!` is in the punctuation class because it is bash's negation token and a
  # command still runs after it: `if ! mapfile -t xs < f` is an invocation. The
  # set originally listed `not` instead, which is not a bash keyword at all — a
  # candidate that could never match, reading to anyone who checked as though
  # negation were covered.
  COMMAND_POSITION = /(?:\A|[;&|(){}!]|\b(?:if|then|else|elif|do|while|until)\b)\s*/.freeze

  # Builtins and expansions that bash 3.2 does not have at all. Every entry was
  # executed against /bin/bash 3.2 and the recorded failure is what it produced.
  BUILTIN_RULES = [
    {
      rule: "associative-array",
      pattern: /#{COMMAND_POSITION}(?:declare|local|typeset)\s+-[A-Za-z]*A[A-Za-z]*\s/,
      detail: "bash 3.2 has no associative arrays: `declare: -A: invalid option`",
      fix: "use a `case` lookup function or parallel indexed arrays"
    },
    {
      rule: "mapfile",
      pattern: /#{COMMAND_POSITION}(?:mapfile|readarray)(?![\w-])/,
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
      pattern: /#{COMMAND_POSITION}(?:declare|local|typeset)\s+-[A-Za-z]*n[A-Za-z]*\s/,
      detail: "bash 3.2 has no name references: `local: -n: invalid option`",
      fix: "pass the value, or use `eval` with an explicit guard"
    },
    {
      rule: "wait-n",
      pattern: /#{COMMAND_POSITION}wait\s+-n(?![\w-])/,
      detail: "bash 3.2 has no `wait -n`: `wait: -n: invalid option`",
      fix: "wait on explicit PIDs"
    },
    {
      rule: "globstar",
      pattern: /#{COMMAND_POSITION}shopt\s+(?:-[A-Za-z]+\s+)*globstar(?![\w-])/,
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
    lines, state = ShellLexer.code_lines(text)

    findings = empty_expansion_findings(relative, lines) + builtin_findings(relative, lines)
    unclosed = unclosed_heredoc_finding(relative, state)
    unclosed ? findings + [unclosed] : findings
  end

  # Per-file line accounting: how many lines the scan read, how many it dropped
  # as here-document bodies, and the last line number it reached.
  #
  # This exists because the FR-138 defect happened while the gate was green. "The
  # gate passes" cannot be evidence that the gate reads the whole file, since a
  # truncated scan is precisely the state it passed in. The census can be false
  # where the exit code cannot.
  #
  # `heredoc` is counted by the lexer as it drops lines, never derived from
  # `total - scanned`. Derived, the sum would be an identity — true of a lexer
  # that stops at line one — and the check would certify an accounting it never
  # performed. `last` is what actually catches truncation: a scan that stops
  # early leaves it below `total`.
  Census = Struct.new(:file, :total, :scanned, :heredoc, :last, keyword_init: true)

  def self.census_file(repo_root, relative)
    text = File.read(repo_root.join(relative))
    lines, state = ShellLexer.code_lines(text)

    Census.new(
      file: relative,
      total: text.lines.length,
      scanned: lines.length,
      heredoc: state.heredoc_lines,
      last: lines.empty? ? 0 : lines.last.first
    )
  end

  def self.census(repo_root)
    shell_files(repo_root).map { |relative| census_file(repo_root, relative) }
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
  census_only = false

  OptionParser.new do |opts|
    opts.banner = "usage: bash32-compat.rb [--list-files] [--coverage-census]"
    opts.on("--list-files", "print the scanned set and exit") { list_only = true }
    opts.on("--coverage-census", "print per-file line accounting and exit") { census_only = true }
    opts.on("--repo-root PATH", "scan a different checkout") { |value| repo_root = Pathname.new(value) }
  end.parse!(ARGV)

  if list_only
    puts Bash32Compat.shell_files(repo_root)
    exit 0
  end

  if census_only
    # `file total scanned heredoc last`, one record per line, in that order.
    Bash32Compat.census(repo_root).each do |record|
      puts "#{record.file} #{record.total} #{record.scanned} #{record.heredoc} #{record.last}"
    end
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
