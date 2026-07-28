#!/usr/bin/env ruby
# frozen_string_literal: true

# FR-133: the dependency policy must still be binding, not merely present.
#
# `cargo deny` proves the policy holds. Nothing proves the policy still *binds*.
# That is a different claim and it is the one this repository keeps finding
# broken: FR-127 learned that wired is not running; FR-137 that an aggregation
# nobody guarded silently swallowed every failure; FR-144 that a gate can print
# PASS over input it could not read. A flag quietly dropped from a `run:` line
# while somebody reformats a long command switches a ratchet off and produces a
# green build that says nothing.
#
# So this gate reads three artefacts and asks whether they still agree:
#
#   deny.toml                        the policy
#   .github/workflows/security.yml   whether CI executes it, and how
#   .cargo/audit.toml                the advisory acceptances
#
# Every rule here has zero violations today. Each is a guard rather than a
# repair, which means its negative fixture in scripts/qa/test-dependency-policy.sh
# is the only evidence it works — a rule nobody has tried to trip is a rule
# nobody knows can fire.
#
# The workflow is *parsed*, through scripts/lib/workflow_model.rb, never grepped:
# a commented-out step satisfies a grep, and FR-134 recorded three separate ways
# that mattered. deny.toml and Cargo.lock are parsed too, by the small TOML
# reader below, for the same reason one level down — counting brackets per line
# is §4.4 shape 3, and this gate's whole subject is not letting a proxy stand
# alone.
#
# `skip-is-live` looks like a re-derivation of `--deny unmatched-skip` and is
# not. Measured: unmatched-skip asks whether the skip matched a crate in the
# graph, so a version that is still present but no longer *duplicated* — the
# shape you get when the graph converges onto the older copy — passes it. That
# case is caught here and nowhere else. It also needs no cargo-deny binary, so
# it runs in the governance job, where there is none.

require "optparse"
require "pathname"
require "shellwords"

require_relative "../lib/workflow_model"

# A TOML reader for the subset these three files use: tables, arrays of tables,
# basic and literal strings, booleans, integers, arrays and inline tables.
#
# It exists because Ruby 2.6 ships no TOML parser and because the alternative —
# regular expressions over lines — is the failure this gate is built to catch.
# It records the source line of every array element so the two rules that need a
# justifying comment can find one.
module PolicyToml
  Document = Struct.new(:tables, :arrays, :element_lines, keyword_init: true)

  class Error < StandardError; end

  module_function

  # => Document
  #   tables:        { "bans" => { "multiple-versions" => "deny", ... } }
  #   arrays:        { "package" => [ {...}, {...} ] }   # from [[package]]
  #   element_lines: { "bans.skip" => [12, 13, ...] }    # 1-based
  def parse(text)
    s = Scanner.new(text)
    tables = {}
    arrays = Hash.new { |h, k| h[k] = [] }
    element_lines = {}
    current = tables[""] = {}
    current_name = ""

    loop do
      s.skip_blanks
      break if s.eof?

      if s.peek == "["
        s.advance
        if s.peek == "["
          s.advance
          name = s.read_key_path
          s.expect("]")
          s.expect("]")
          current = {}
          arrays[name] << current
          current_name = name
        else
          name = s.read_key_path
          s.expect("]")
          current = tables[name] ||= {}
          current_name = name
        end
        next
      end

      key = s.read_key_path
      s.skip_inline
      s.expect("=")
      line = s.line
      value = s.read_value
      current[key] = value
      element_lines["#{current_name}.#{key}"] = s.take_element_lines if value.is_a?(Array)
      _ = line
    end

    Document.new(tables: tables, arrays: arrays, element_lines: element_lines)
  end

  # Character scanner. Comments are recognised only outside strings, which is
  # the whole reason this is a scanner and not a `gsub(/#.*/, "")`.
  class Scanner
    def initialize(text)
      @text = text
      @pos = 0
      @line = 1
      @element_lines = []
    end

    attr_reader :line

    def eof?
      @pos >= @text.length
    end

    def peek
      @text[@pos]
    end

    def advance
      @line += 1 if @text[@pos] == "\n"
      @pos += 1
    end

    def take_element_lines
      lines = @element_lines
      @element_lines = []
      lines
    end

    def skip_inline
      advance while !eof? && (peek == " " || peek == "\t")
    end

    def skip_blanks
      loop do
        skip_inline
        if peek == "#"
          advance while !eof? && peek != "\n"
        elsif peek == "\n" || peek == "\r"
          advance
        else
          return
        end
      end
    end

    def expect(char)
      skip_blanks
      raise Error, "line #{@line}: expected #{char.inspect}, found #{peek.inspect}" unless peek == char

      advance
    end

    def read_key_path
      skip_blanks
      out = +""
      loop do
        if peek == '"'
          out << read_basic_string
        elsif peek =~ /[A-Za-z0-9_.\-]/
          out << peek
          advance
        else
          break
        end
      end
      raise Error, "line #{@line}: empty key" if out.empty?

      out
    end

    def read_value
      skip_blanks
      case peek
      when '"' then read_basic_string
      when "'" then read_literal_string
      when "[" then read_array
      when "{" then read_inline_table
      else read_bare
      end
    end

    def read_basic_string
      expect('"')
      out = +""
      until peek == '"'
        raise Error, "line #{@line}: unterminated string" if eof?

        if peek == "\\"
          advance
          out << unescape(peek)
        else
          out << peek
        end
        advance
      end
      advance
      out
    end

    def unescape(char)
      { "n" => "\n", "t" => "\t", "r" => "\r", '"' => '"', "\\" => "\\" }.fetch(char, char)
    end

    def read_literal_string
      expect("'")
      out = +""
      until peek == "'"
        raise Error, "line #{@line}: unterminated literal string" if eof?

        out << peek
        advance
      end
      advance
      out
    end

    def read_array
      expect("[")
      items = []
      loop do
        skip_blanks
        break if peek == "]"
        raise Error, "line #{@line}: unterminated array" if eof?

        @element_lines << @line
        items << read_value
        skip_blanks
        advance if peek == ","
      end
      advance
      items
    end

    def read_inline_table
      expect("{")
      table = {}
      loop do
        skip_blanks
        break if peek == "}"
        raise Error, "line #{@line}: unterminated inline table" if eof?

        key = read_key_path
        skip_blanks
        expect("=")
        table[key] = read_value
        skip_blanks
        advance if peek == ","
      end
      advance
      table
    end

    def read_bare
      out = +""
      until eof? || peek =~ /[,\]}\n#]/
        out << peek
        advance
      end
      raw = out.strip
      case raw
      when "true" then true
      when "false" then false
      when /\A-?\d+\z/ then raw.to_i
      else raw
      end
    end
  end
end

module DependencyPolicy
  DENY = "deny.toml"
  WORKFLOW = ".github/workflows/security.yml"
  AUDIT = ".cargo/audit.toml"
  LOCK = "Cargo.lock"
  CHECKS = %w[bans licenses sources].freeze

  Finding = Struct.new(:file, :rule, :detail, :fix, keyword_init: true)

  module_function

  # ── Reading ────────────────────────────────────────────────────────────────

  def read_toml(root, relative)
    path = root.join(relative)
    return [nil, "#{relative} does not exist"] unless path.file?

    [PolicyToml.parse(path.read), nil]
  rescue PolicyToml::Error => e
    [nil, "#{relative} could not be parsed: #{e.message}"]
  end

  # The command lines of every job, keyed by job. Each entry is one physical
  # line, because a `run:` block may hold several and only one of them is the
  # invocation being asked about.
  def command_lines(root)
    path = root.join(WORKFLOW)
    return {} unless path.file?

    WorkflowModel.jobs(path.to_s).to_h do |job|
      # run_commands returns one joined string per job, already stripped of
      # comments and heredoc bodies by WorkflowModel.executable_shell — so a
      # commented-out invocation is invisible here, which is the point.
      lines = WorkflowModel.run_commands(path.to_s, job)
                           .to_s.lines.map(&:strip).reject(&:empty?)
      [job, lines]
    end
  end

  # The tokens of the first line in `lines` that invokes `cargo deny` /
  # `cargo-deny`, or nil. Splitting is Shellwords rather than a regex so that
  # quoting behaves the way the runner's shell will.
  def invocation(lines, *heads)
    lines.each do |line|
      tokens = begin
        Shellwords.split(line)
      rescue ArgumentError
        line.split
      end
      next if tokens.empty?

      heads.each do |head|
        parts = head.split
        return tokens if tokens.first(parts.length) == parts
      end
    end
    nil
  end

  # Positional arguments after the `check` subcommand: the check names. Flags and
  # the values of flags that take one are dropped, so argument order cannot
  # change the answer.
  def check_names(tokens)
    index = tokens.index("check")
    return [] unless index

    rest = tokens[(index + 1)..] || []
    names = []
    skip_next = false
    rest.each do |token|
      if skip_next
        skip_next = false
        next
      end
      if token.start_with?("-")
        skip_next = %w[-D --deny -W --warn -A --allow -c --config -g --graph
                       -f --format --feature-depth].include?(token)
        next
      end
      names << token
    end
    names
  end

  def flag?(tokens, flag, value)
    tokens.each_cons(2) { |a, b| return true if a == flag && b == value }
    tokens.include?("#{flag}=#{value}")
  end

  # ── Rules ──────────────────────────────────────────────────────────────────

  def check_workflow(root, findings)
    jobs = command_lines(root)

    # §4.4 shape 5: a workflow that yields no jobs must not read as a clean pass.
    # Everything below is vacuously satisfied over an empty set.
    if jobs.empty?
      findings << Finding.new(
        file: WORKFLOW, rule: "empty-scan",
        detail: "the workflow yielded no jobs, so every rule below examined nothing",
        fix: "a clean result over an empty workflow is a property of the parse, not of CI"
      )
      return
    end

    deny_job, deny_tokens = jobs.map { |job, lines|
      tokens = invocation(lines, "cargo deny", "cargo-deny")
      tokens ? [job, tokens] : nil
    }.compact.first

    if deny_tokens.nil?
      findings << Finding.new(
        file: WORKFLOW, rule: "deny-job-present",
        detail: "no job runs `cargo deny`, so #{DENY} is a document rather than a policy",
        fix: "add the cargo-deny job back; a policy that cannot fail the build enforces nothing"
      )
      return
    end

    swallowed = WorkflowModel.continue_on_error_steps(root.join(WORKFLOW).to_s, deny_job)
    unless swallowed.empty?
      findings << Finding.new(
        file: WORKFLOW, rule: "deny-job-present",
        detail: "job '#{deny_job}' declares continue-on-error on #{swallowed.length} step(s), so a policy violation cannot fail it",
        fix: "drop continue-on-error, or add an OUTCOMES aggregation the way ci.yml's governance job does"
      )
    end

    unless flag?(deny_tokens, "--deny", "unmatched-skip")
      findings << Finding.new(
        file: WORKFLOW, rule: "ratchet-armed",
        detail: "job '#{deny_job}' runs cargo deny without `--deny unmatched-skip`, so an accepted duplicate that upstream has resolved keeps its entry silently",
        fix: "restore `--deny unmatched-skip`; it is the only thing that makes the skip list able to shrink"
      )
    end

    names = check_names(deny_tokens)
    if names.sort != CHECKS.sort
      findings << Finding.new(
        file: WORKFLOW, rule: "checks-partitioned",
        detail: "job '#{deny_job}' checks #{names.inspect}, not #{CHECKS.inspect}",
        fix: "cargo-deny owns graph shape and cargo audit owns the advisory database; `advisories` or `all` here double-reports the 17 unmaintained findings and still misses the unsound class cargo-deny does not surface"
      )
    end

    audit_tokens = jobs.values.map { |lines| invocation(lines, "cargo audit", "cargo-audit") }.compact.first
    if audit_tokens.nil?
      findings << Finding.new(
        file: WORKFLOW, rule: "audit-unsound-denied",
        detail: "no job runs `cargo audit`, and cargo-deny does not report unsound advisories",
        fix: "the split only works while both halves run"
      )
    elsif !flag?(audit_tokens, "--deny", "unsound")
      findings << Finding.new(
        file: WORKFLOW, rule: "audit-unsound-denied",
        detail: "cargo audit runs without `--deny unsound`, so it exits 0 over unsoundness findings the way it did over RUSTSEC-2024-0429",
        fix: "restore `--deny unsound`; without it the acceptances in #{AUDIT} are decoration"
      )
    end
  end

  def check_policy(root, findings)
    doc, error = read_toml(root, DENY)
    if doc.nil?
      findings << Finding.new(file: DENY, rule: "severity-binding", detail: error,
                              fix: "the policy file is the subject of every rule below")
      return nil
    end

    {
      %w[bans multiple-versions] => "deny",
      %w[licenses unused-allowed-license] => "deny",
      %w[sources unknown-registry] => "deny",
      %w[sources unknown-git] => "deny"
    }.each do |(table, key), expected|
      actual = doc.tables.dig(table, key)
      next if actual == expected

      findings << Finding.new(
        file: DENY, rule: "severity-binding",
        detail: "[#{table}] #{key} is #{actual.inspect}, not #{expected.inspect}",
        fix: "anything other than \"deny\" turns this check advisory, and an advisory check reports the same green as a passing one"
      )
    end

    skip_tree = doc.tables.dig("bans", "skip-tree")
    if skip_tree.is_a?(Array) && !skip_tree.empty?
      findings << Finding.new(
        file: DENY, rule: "no-blanket",
        detail: "[bans] skip-tree has #{skip_tree.length} entr#{skip_tree.length == 1 ? 'y' : 'ies'}",
        fix: "a skip-tree accepts duplicates that do not exist yet, forever and silently; list the crates instead, one reason each"
      )
    end

    skips = doc.tables.dig("bans", "skip") || []
    skips.each do |entry|
      next unless entry.is_a?(Hash)

      reason = entry["reason"].to_s.strip
      next unless reason.empty?

      findings << Finding.new(
        file: DENY, rule: "every-acceptance-reasoned",
        detail: "the skip for #{entry['crate'].inspect} carries no reason",
        fix: "an accepted duplicate without a reason is an unreviewed one wearing a reviewed one's clothes"
      )
    end

    check_exception_comments(root, doc, findings)
    doc
  end

  # `exceptions` entries accept only `crate` and `allow` — cargo-deny rejects a
  # `reason` key there — so the justification has to be a comment, and this is
  # what checks that one exists. Same treatment as .cargo/audit.toml below.
  def check_exception_comments(root, doc, findings)
    exceptions = doc.tables.dig("licenses", "exceptions") || []
    return if exceptions.empty?

    lines = root.join(DENY).readlines
    starts = doc.element_lines["licenses.exceptions"] || []
    exceptions.each_with_index do |entry, index|
      line = starts[index]
      next if line.nil?
      next if commented_above?(lines, line)

      findings << Finding.new(
        file: DENY, rule: "every-acceptance-reasoned",
        detail: "the licence exception for #{entry['crate'].inspect} has no comment above it",
        fix: "cargo-deny rejects a `reason` key on exceptions, so the comment is the only place the justification can live"
      )
    end
  end

  # True when the lines immediately above `line` (1-based) include a comment,
  # crossing blank lines but not other content.
  def commented_above?(lines, line)
    index = line - 2
    while index >= 0
      text = lines[index].to_s.strip
      return true if text.start_with?("#")
      return false unless text.empty?

      index -= 1
    end
    false
  end

  # Every skip must name a crate that really has more than one version in the
  # lock, at the version written. The third branch below is the one cargo-deny
  # cannot reach: `--deny unmatched-skip` is satisfied by a crate that exists,
  # duplicated or not.
  def check_skips_live(root, doc, findings)
    return if doc.nil?

    lock, error = read_toml(root, LOCK)
    if lock.nil?
      findings << Finding.new(file: LOCK, rule: "skip-is-live", detail: error,
                              fix: "without the lock there is nothing to check the skip list against")
      return
    end

    packages = lock.arrays["package"] || []
    if packages.empty?
      findings << Finding.new(
        file: LOCK, rule: "empty-scan",
        detail: "the lock yielded no packages, so skip-is-live examined nothing",
        fix: "every skip entry is vacuously live against an empty lock"
      )
      return
    end

    versions = packages.group_by { |p| p["name"] }.transform_values { |ps| ps.map { |p| p["version"] } }

    (doc.tables.dig("bans", "skip") || []).each do |entry|
      next unless entry.is_a?(Hash)

      spec = entry["crate"].to_s
      name, version = spec.split("@", 2)
      known = versions[name]

      if known.nil?
        findings << Finding.new(
          file: DENY, rule: "skip-is-live",
          detail: "#{spec} skips a crate that is not in #{LOCK} at all",
          fix: "delete the entry; it accepts nothing and hides the next crate to take that name"
        )
      elsif version && !known.include?(version)
        findings << Finding.new(
          file: DENY, rule: "skip-is-live",
          detail: "#{spec} names a version that is not in #{LOCK} (found #{known.sort.join(', ')})",
          fix: "the duplicate this entry accepted has moved; re-derive it from cargo deny's output"
        )
      elsif known.length < 2
        findings << Finding.new(
          file: DENY, rule: "skip-is-live",
          detail: "#{spec} skips a crate that resolves to exactly one version, so it accepts a duplicate that no longer exists",
          fix: "delete the entry; this is what `--deny unmatched-skip` fails on in CI, observed here without the binary"
        )
      end
    end
  end

  # Every ignored advisory needs a justification, and an ignore file has nowhere
  # to put one except a comment.
  def check_audit(root, findings)
    doc, error = read_toml(root, AUDIT)
    if doc.nil?
      findings << Finding.new(file: AUDIT, rule: "audit-unsound-denied", detail: error,
                              fix: "`--deny unsound` without an acceptance file fails on the first informational advisory")
      return
    end

    ignored = doc.tables.dig("advisories", "ignore") || []
    lines = root.join(AUDIT).readlines
    starts = doc.element_lines["advisories.ignore"] || []

    ignored.each_with_index do |id, index|
      line = starts[index]
      next if line.nil?
      next if commented_above?(lines, line)

      findings << Finding.new(
        file: AUDIT, rule: "audit-unsound-denied",
        detail: "#{id} is ignored with no comment above it",
        fix: "an advisory accepted without a reason and a retirement condition is one nobody will ever remove"
      )
    end
  end

  def run(root)
    findings = []
    check_workflow(root, findings)
    doc = check_policy(root, findings)
    check_skips_live(root, doc, findings)
    check_audit(root, findings)
    [doc, findings]
  end
end

if $PROGRAM_NAME == __FILE__
  repo_root = Pathname.new(File.expand_path("../..", __dir__))

  OptionParser.new do |opts|
    opts.banner = "usage: dependency-policy.rb [--repo-root PATH]"
    opts.on("--repo-root PATH", "check a different checkout") { |value| repo_root = Pathname.new(value) }
  end.parse!(ARGV)

  doc, findings = DependencyPolicy.run(repo_root)

  findings.each do |finding|
    warn "#{finding.file}: [#{finding.rule}] #{finding.detail}"
    warn "  fix: #{finding.fix}"
  end

  accepted = (doc&.tables&.dig("bans", "skip") || []).length

  if findings.empty?
    puts "Dependency policy: PASS (#{accepted} accepted duplicate(s), 0 finding(s))"
    exit 0
  end

  warn ""
  warn "Dependency policy: FAIL (#{accepted} accepted duplicate(s), #{findings.length} finding(s))"
  exit 1
end
