#!/usr/bin/env ruby
# frozen_string_literal: true

# FR-145: under `set -o pipefail`, a reader that leaves early kills the producer,
# and the producer's death becomes the pipeline's answer.
#
# The shape:
#
#   printf '%s' "$UNRELEASED" | rg -q 'RunnerExecutorKind' || fail "..."
#
# `rg -q` exits on the first match. When the producer has more than a pipe buffer
# left to write, it is still writing when the reader leaves, dies of EPIPE, and
# `set -o pipefail` hands that non-zero status to the caller. The assertion then
# reports the opposite of what it observed.
#
# Measured on the real `CHANGELOG.md` `[Unreleased]` section (90047 bytes, match
# at byte 59273), 400 iterations per row, at f105ce66:
#
#            printf | rg -q     8/400 idle,  10/400 under load
#            printf | grep -q   3/400 idle,   2/400 under load
#            rg -q P <<< "$U"   0/400 idle,   0/400 under load
#
# Two things about that table decide the shape of this gate.
#
# First, **the direction is not fixed**. Where the match feeds the pass branch,
# EPIPE produces a false failure — a mystery red, which people re-run until it
# is green. Where the match feeds the *fail* branch, EPIPE produces a false
# pass: a real violation reported as clean, measured at 2/200. The repository's
# one unbounded producer was in the second class, asserting that provider
# session material had not leaked into a database dump.
#
# Second, **"can this producer exceed the buffer" is not derivable**. A 90 KB
# producer with the match at 59 KB fires 2-3% of the time; a 1 MB single-line
# producer with the match at byte 0 fired 0/200. Match position and line
# structure decide it, `rg` and BSD `grep` differ, and in any case producer size
# is a property of today's *data*: the CHANGELOG took years to cross 64 KB. An
# exemption reading "this producer is bounded" is a claim nothing re-checks.
#
# So the rule is syntactic and has **no escape hatch**. The alternative spelling
#
#   grep -q PATTERN <<< "$(producer)"
#
# writes a temporary file, leaves no writer to signal, and is correct at every
# size, every match position and every implementation of grep. A rule whose
# remedy is always available does not need an exemption, and an exemption is how
# a rule gets quietly widened (SKILL.md §4.4 shape 8).
#
# Scope is every tracked `*.sh`, with no exemption — not even for a file that
# does not enable `pipefail` itself.
#
# The first version of this scanner did exempt those, and it was wrong, because
# **shell options are dynamic, not lexical**. `scripts/regression/run-cli-probes.sh`
# sets `-euo pipefail` and then `source`s each file under `scenarios/`; those
# files enable nothing themselves and their pipelines run under the runner's
# options anyway. Demonstrated: a scenario sourced into a pipefail runner reports
# `NOT matched` on a pattern that is present. Two live sites were outside the
# governed set for exactly that reason, and the FR that produced this scanner had
# recorded both as immune.
#
# Proving "nothing sources this file" is not something a scanner can do — the
# sourcing site is `source "$scenario_script"`, a variable. So the exemption goes,
# the same way the per-site one did: the remedy costs nothing and is correct
# everywhere, so there is no state worth exempting.
#
# It is also deliberately broader than `config/governance/qa-gate-surface.json`,
# because the hazard has nothing to do with whether a script is ci-required — and
# because `scripts/qa-doc-lint.sh`, `scripts/coverage-governance.sh` and
# `scripts/check-async-lock-governance.sh` are executed by `ci.yml` while absent
# from that manifest, so a manifest-derived scope would miss the invoker of the
# run where this defect was first observed.
#
# Comments, single-quoted regions and here-document bodies come off via
# `scripts/lib/shell_lexer.rb` before anything is matched, and the pipe split is
# quote-aware. Both are load-bearing: FR-145 was filed with a count of 42 sites
# that was really 35, because `grep -c` counted four comment lines *describing*
# the pattern — one of them the comment written to explain the first fix. A gate
# that repeats that error reports findings on its own documentation and gets
# switched off.

require "open3"
require "optparse"
require "pathname"

require_relative "../lib/shell_lexer"

module PipefailShortCircuit
  Finding = Struct.new(:file, :line, :rule, :detail, :fix, keyword_init: true)

  # Readers that can leave before end of input, in two families.
  #
  # `grep`/`rg` only short-circuit when told to: `-q`, `--quiet`, `--silent`, `-m N`. Without one
  # of those they read to end of input and are harmless downstream of anything.
  MATCH_READERS = %w[grep rg egrep fgrep ggrep].freeze
  #
  # `head` always short-circuits — there is no flag that makes it read to the end — so there is no
  # flag test for it. FR-146 measured it at 37 sites across 29 files, and it fires far harder than
  # the `-q` family: with `X="$(seq 1 N | head -1)"` under `set -euo pipefail`, 0/10 deaths at
  # 3.9 KB, **6/10 at 24 KB, 10/10 at 129 KB and above**, where `grep -q` on a 90 KB producer
  # managed 8-13 in 400. The consequence differs too. `-q` sites sit in conditions and invert an
  # assertion; `head` sites are assignments and bare commands, whose status reaches `set -e` and
  # ends the run **with no summary line** — the shape §4.4 shape 7 names, where a truncated run
  # reads exactly like a complete one.
  #
  # The remedy is not a flag either: `sed -n '1,Np'` and `awk 'NR<=N'` read to end of input, and
  # where the intent is "the first line of a captured result", `${out%%$'\n'*}` needs no pipe at
  # all. All three were measured against a 1.3 MB producer.
  ALWAYS_READERS = %w[head].freeze
  READERS = (MATCH_READERS + ALWAYS_READERS).freeze


  module_function

  # Coverage is whatever git tracks, so a script added tomorrow is scanned
  # tomorrow and no roster has to be remembered.
  def shell_files(repo_root)
    output, status = Open3.capture2("git", "-C", repo_root.to_s, "ls-files", "-z", "*.sh")
    raise "git ls-files failed" unless status.success?

    output.split("\0").reject(&:empty?).sort
  end

  # The governed set is the tracked set. There is no `pipefail?` predicate here on
  # purpose: whether the option is in force at a given pipeline is a property of
  # the *running shell*, and this repository sources four files into shells that
  # set it. See the header.
  def governed_files(repo_root)
    shell_files(repo_root)
  end

  # ── Pipe splitting ──────────────────────────────────────────────────────────

  # Quoting context, carried across lines, mirroring ShellLexer::State's reason
  # for being a stack: `$(` opens a fresh one. Without that, the `|` in
  # `X="$(cmd | grep -q y)"` reads as sitting inside double quotes and the stage
  # is never seen — and a command substitution is one of the two places this
  # shape actually appears in the repository.
  #
  # Single-quoted regions arrive already blanked by the lexer, so only double
  # quotes are tracked here.
  class Quoting
    def initialize
      @stack = [{ dq: false, depth: 0 }]
      @pending_pipe = false
    end

    attr_accessor :pending_pipe

    def top
      @stack.last
    end

    def dq?
      top[:dq]
    end

    def toggle_dq
      top[:dq] = !top[:dq]
    end

    def open_substitution
      @stack.push(dq: false, depth: 0)
    end

    def close_substitution
      @stack.pop if @stack.length > 1
    end

    def depth_up
      top[:depth] += 1
    end

    # True when this `)` closed a nested substitution rather than a plain group.
    def depth_down
      if top[:depth].positive?
        top[:depth] -= 1
        false
      else
        close_substitution
        true
      end
    end
  end

  # Split one code line into pipeline stages on unquoted `|`.
  #
  # `||` is not a pipe and must not split — the repository writes
  # `grep -q x file || fail "..."` constantly, and splitting there would report
  # the reader as a downstream stage of nothing.
  #
  # Returns the stages as strings, in order. `state` is advanced.
  def stages(code, state)
    parts = [+""]
    index = 0

    while index < code.length
      char = code[index]

      if char == "\\"
        parts.last << char << (code[index + 1] || "")
        index += 2
        next
      end

      if char == '"'
        state.toggle_dq
        parts.last << char
        index += 1
        next
      end

      # Deliberately *not* guarded by the double-quote test. `"$( … )"` opens a
      # fresh quoting context inside a double-quoted region, which is why
      # ShellLexer treats `$(` the same way. Guarding it here made the `|` in
      # `X="$(cat f | grep -q inner)"` read as quoted, and the stage vanished.
      if char == "$" && code[index + 1] == "(" && code[index + 2] != "("
        state.open_substitution
        parts.last << char << "("
        index += 2
        next
      end

      unless state.dq?
        if char == "("
          state.depth_up
        elsif char == ")"
          state.depth_down
        elsif char == "|"
          # `||` — a list operator, not a pipe.
          if code[index + 1] == "|"
            parts.last << "||"
            index += 2
            next
          end

          # `|&` pipes stdout and stderr; still a pipe.
          parts << +""
          index += (code[index + 1] == "&" ? 2 : 1)
          next
        end
      end

      parts.last << char
      index += 1
    end

    parts
  end

  # ── Reader detection ────────────────────────────────────────────────────────

  # The command word of a pipeline stage: leading whitespace, subshell and group
  # openers, and environment assignments removed. `LC_ALL=C sort` runs `sort`.
  def command_word(segment)
    rest = segment.sub(/\A[\s(){]*/, "")
    loop do
      token = rest[/\A\S+/]
      return nil if token.nil?
      break unless token =~ /\A[A-Za-z_][A-Za-z0-9_]*=/

      rest = rest[token.length..-1].sub(/\A\s*/, "")
    end

    token = rest[/\A\S+/]
    token.nil? ? nil : File.basename(token.delete("\"'"))
  end

  # A stage carries past the reader's own arguments: a pipeline segment ends
  # where the enclosing construct does, and the tail belongs to something else.
  # Scanning it for flags reads `[[ "$(… | grep -c .)" -eq 3 ]]` as a quiet grep,
  # because `-eq` matches "a short flag cluster containing q". That was three
  # false positives on the first run over this repository, all of them `grep -c`
  # — a reader that counts, and therefore reads to end of input.
  STAGE_END = /[);]|\A(?:&&|\|\||\]\]|\]|then|do|fi|done)\z/.freeze

  # Whether a stage invoking one of READERS carries a flag that makes it leave on
  # the first match. `--` ends the option list, so `grep -F -- -q` is a pattern
  # and not a quiet flag — the case a regex over the whole line gets wrong.
  def quiet_flag(segment)
    tokens = segment.split(/\s+/).reject(&:empty?)
    tokens.shift # the command word itself, already identified

    tokens.each do |token|
      break if token == "--" || token =~ STAGE_END
      return token if %w[--quiet --silent].include?(token)
      # A short-flag cluster carrying `q`: -q, -qxF, -aq.
      return token if token =~ /\A-[A-Za-z]*q[A-Za-z]*\z/
      # `-m N` / `-m1` / `--max-count` stop after N matches, which is the same
      # act. Zero sites today; it costs nothing and closes the obvious rewrite.
      return token if token =~ /\A-[A-Za-z]*m[0-9]*\z/ || token.start_with?("--max-count")
    end

    nil
  end

  def reader_stage(segment)
    word = command_word(segment)
    return [word, nil] if ALWAYS_READERS.include?(word)
    return nil unless MATCH_READERS.include?(word)

    flag = quiet_flag(segment.sub(/\A[\s(){]*/, ""))
    flag && [word, flag]
  end

  # ── Scanning ────────────────────────────────────────────────────────────────

  def scan_file(repo_root, relative)
    lines, lexer_state = ShellLexer.code_lines(File.read(repo_root.join(relative)))

    # A file that ends inside a here-document was never fully read, so a clean
    # result over it means nothing. Reported rather than assumed away, for the
    # same reason bash32-compat.rb reports it: the truncated scan is exactly the
    # state a green run would be hiding.
    if lexer_state.in_heredoc?
      return [Finding.new(
        file: relative, line: lexer_state.heredoc_line, rule: "unclosed-heredoc",
        detail: "this file ends inside a here-document opened here with terminator " \
                "`#{lexer_state.heredoc}`; every line after it was dropped, so the rest of the " \
                "file is unscanned",
        fix: "close the here-document, or check what quoting the `<<` sits inside"
      )]
    end

    findings = []
    state = Quoting.new

    lines.each do |number, code|
      parts = stages(code, state)

      parts.each_with_index do |segment, position|
        # Position 0 is the head of the pipeline on this line. It is a downstream
        # stage only when the previous line left a pipe open, which happens when
        # a long pipeline is broken after the `|`.
        downstream = position.positive? || (position.zero? && state.pending_pipe)
        next unless downstream

        found = reader_stage(segment)
        next unless found

        reader, flag = found
        named = flag ? "#{reader} #{flag}" : reader
        findings << Finding.new(
          file: relative, line: number, rule: "short-circuit-under-pipefail",
          detail: "`#{named}` leaves before end of input while the producer upstream of " \
                  "it may still be writing; under `set -o pipefail` the producer's EPIPE becomes " \
                  "the pipeline's status. In a condition that inverts the assertion; in an " \
                  "assignment or a bare command it reaches `set -e` and ends the run with no " \
                  "summary line",
          fix: flag ?
            "#{named} PATTERN <<< \"$(producer)\" — a here-string writes a file, so there is no " \
            "writer left to signal. Where a list is built by word splitting, keep it: " \
            "<<< \"$(printf '%s\\n' $list)\"" :
            "`sed -n '1,Np'` or `awk 'NR<=N'` read to end of input; where the intent is the first " \
            "line of a captured result, `out=\"$(producer)\"; first=\"${out%%$'\\n'*}\"` needs no " \
            "pipe at all"
        )
      end

      # `cmd |` at end of line continues the pipeline onto the next one.
      state.pending_pipe = !parts.last.nil? && parts.last.strip.empty? && parts.length > 1
    end

    findings
  end

  def run(repo_root)
    files = shell_files(repo_root)
    findings = files.flat_map { |relative| scan_file(repo_root, relative) }
    [files, findings]
  end
end

if $PROGRAM_NAME == __FILE__
  repo_root = Pathname.new(File.expand_path("../..", __dir__))
  list_only = false

  OptionParser.new do |opts|
    opts.banner = "usage: pipefail-short-circuit.rb [--list-files] [--repo-root PATH]"
    opts.on("--list-files", "print the governed set (every tracked *.sh) and exit") do
      list_only = true
    end
    opts.on("--repo-root PATH", "scan a different checkout") { |value| repo_root = Pathname.new(value) }
  end.parse!(ARGV)

  if list_only
    puts PipefailShortCircuit.governed_files(repo_root)
    exit 0
  end

  files, findings = PipefailShortCircuit.run(repo_root)

  findings.each do |finding|
    warn "#{finding.file}:#{finding.line}: [#{finding.rule}] #{finding.detail}"
    warn "  fix: #{finding.fix}"
  end

  summary = "#{files.length} tracked shell file(s) scanned"

  if findings.empty?
    puts "pipefail short-circuit: PASS (#{summary}, 0 finding(s))"
    exit 0
  end

  warn ""
  warn "pipefail short-circuit: FAIL (#{summary}, #{findings.length} finding(s))"
  exit 1
end
