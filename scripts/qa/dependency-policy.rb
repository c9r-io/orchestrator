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

require "find"
require "optparse"
require "pathname"
require "shellwords"
require "yaml"

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
  DEPENDABOT = ".github/dependabot.yml"
  CHECKS = %w[bans licenses sources].freeze

  # The two counts deny.toml's prose states about itself, as anchored phrases.
  # A phrase that has been reworded out of existence is a failed assertion, not
  # a skip: a gate that cannot find its subject must say so (§4.4 shape 7).
  DUP_PHRASE = /(\d+) crates resolve to more than one version; (\d+) extra copies/
  EXT_PHRASE = /(\d+) external packages/

  # Directories never holding a dependency tree of ours: dependency installs,
  # build output, VCS internals. Everything else is walked — the portal
  # template under .claude/ is a real npm tree and stays in scope.
  PRUNED_DIRS = %w[node_modules target .git dist].freeze

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
    elsif !flag?(audit_tokens, "--deny", "unmaintained")
      findings << Finding.new(
        file: WORKFLOW, rule: "audit-unsound-denied",
        detail: "cargo audit runs without `--deny unmaintained`, so the eighteenth unmaintained advisory arrives as unbooked debt the way the first seventeen did before FR-153",
        fix: "restore `--deny unmaintained`; the ledger in #{AUDIT} only binds while the flag makes an unbooked advisory red"
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

    check_audit_retirement(root, doc, lines, ignored, starts, findings)
  end

  # Every accepted advisory must still be accepting something.
  #
  # `check_audit` above asks only whether an ignore has *a* comment, which is a
  # §4.4 shape 1 proxy for the thing that matters: the comments state retirement
  # conditions in prose, for a human, and nobody read them. An acceptance whose
  # crate left the tree therefore stayed forever — accepting nothing, and holding
  # the advisory ID reserved against the day something else brings that crate
  # back. cargo-audit has no `--deny unmatched-ignore`; `cargo deny --deny
  # unmatched-skip` is the nearest thing and lives on the other file.
  #
  # The condition is declared per entry rather than inferred from the prose, and
  # the difference matters. The gtk block states `cargo tree -i gtk` once above
  # eleven entries, each of which then says "retires with the block condition
  # above" — so walking up from an entry reaches its own one-line comment and
  # never the group's command. Parsing that indirection would be reading a
  # paragraph's structure as data. A `# retire-when:` line above each entry is a
  # declaration instead, in one shape, and an entry without one fails: that is
  # the ratchet, since a new acceptance cannot be added without saying what would
  # end it.
  #
  # Two forms, and which one applies is the advisory's kind rather than a
  # preference. `absent` is the only condition an *unmaintained* advisory can
  # have — the crate is archived, that is the advisory. `patched>=` exists for the
  # kinds that have a fix, and it is the direction presence-checking cannot see:
  # glib reaching 0.20 retires RUSTSEC-2024-0429 while glib stays in the lock, so
  # a gate that only asked "is the crate here" would go on accepting a fixed
  # advisory indefinitely. That is the same half `--deny unmatched-skip` misses on
  # the deny side, recorded at FR-133 as case 15b, and requirement 4 exists partly
  # so it does not repeat here.
  def check_audit_retirement(root, doc, lines, ignored, starts, findings)
    lock, error = read_toml(root, LOCK)
    if lock.nil?
      findings << Finding.new(file: LOCK, rule: "audit-ignore-is-live", detail: error,
                              fix: "without the lock there is nothing to check the acceptances against")
      return
    end

    packages = lock.arrays["package"] || []
    if packages.empty?
      findings << Finding.new(
        file: LOCK, rule: "empty-scan",
        detail: "the lock yielded no packages, so audit-ignore-is-live examined nothing",
        fix: "every acceptance is vacuously live against an empty lock"
      )
      return
    end

    versions = packages.group_by { |p| p["name"] }.transform_values { |ps| ps.map { |p| p["version"] } }

    ignored.each_with_index do |id, index|
      line = starts[index]
      next if line.nil?

      condition = retirement_condition(lines, line)
      if condition.nil?
        findings << Finding.new(
          file: AUDIT, rule: "audit-ignore-is-live",
          detail: "#{id} has no `# retire-when:` line above it",
          fix: "add `# retire-when: crate=<name> absent` (unmaintained) or " \
               "`# retire-when: crate=<name> patched>=<version>`; an acceptance whose end " \
               "condition is only prose is one nothing can ever retire"
        )
        next
      end

      crate, bound = condition
      known = versions[crate]

      if known.nil?
        findings << Finding.new(
          file: AUDIT, rule: "audit-ignore-is-live",
          detail: "#{id} accepts an advisory against #{crate}, which is not in #{LOCK} at all",
          fix: "delete the entry; it accepts nothing, and it reserves the advisory against " \
               "whatever brings #{crate} back"
        )
        next
      end

      next if bound.nil?

      unpatched = known.reject { |version| version_at_least?(version, bound) }
      next unless unpatched.empty?

      findings << Finding.new(
        file: AUDIT, rule: "audit-ignore-is-live",
        detail: "#{id} is accepted until #{crate} reaches #{bound}, and the lock has " \
                "#{known.sort.join(', ')} — the advisory is fixed",
        fix: "delete the entry; the crate is still in the tree, which is exactly the half " \
             "a presence check cannot see"
      )
    end
  end

  # `# retire-when: crate=<name> absent` or `... patched>=<version>`, taken from
  # the comment block immediately above `line`. Returns [crate, bound_or_nil].
  def retirement_condition(lines, line)
    index = line - 2
    while index >= 0
      text = lines[index].to_s.strip
      if (match = text.match(/\A#\s*retire-when:\s*crate=(\S+)\s+(absent|patched>=(\S+))\s*\z/))
        return [match[1], match[3]]
      end
      break unless text.start_with?("#") || text.empty?

      index -= 1
    end
    nil
  end

  # Numeric, component-wise. `"0.18.5" >= "0.20.0"` is false here and true under
  # string comparison, which is the whole reason this is not a `>=` on strings:
  # "18" sorts after "20" lexically, so the naive form would report every glib
  # 0.18.x as patched and delete the acceptance that is doing its job.
  def version_at_least?(version, bound)
    actual = version.split(/[.+-]/).map(&:to_i)
    target = bound.split(/[.+-]/).map(&:to_i)
    depth = [actual.length, target.length].max
    (0...depth).each do |i|
      a = actual[i] || 0
      b = target[i] || 0
      return true if a > b
      return false if a < b
    end
    true
  end

  # deny.toml states counts about itself — 48 crates / 71 copies from its own
  # skip list, 654 external packages from the lock. FR-153 found the copy count
  # one day stale (base64@0.22.1 landed after the sentence was written), so the
  # prose is now compared against the artefacts it describes instead of being
  # trusted. The derivations are the file's own tables: no graph resolution is
  # needed, which is what lets this run without cargo.
  def check_prose_counts(root, doc, findings)
    return if doc.nil?

    text = root.join(DENY).read
    skips = (doc.tables.dig("bans", "skip") || []).select { |e| e.is_a?(Hash) }
    copies = skips.length
    crates = skips.map { |e| e["crate"].to_s.split("@", 2).first }.uniq.length

    match = text.match(DUP_PHRASE)
    if match.nil?
      findings << Finding.new(
        file: DENY, rule: "prose-counts-derived",
        detail: "the header no longer states the duplicate counts (expected the phrase 'N crates resolve to more than one version; M extra copies')",
        fix: "restore the sentence; a count this rule cannot find is a count it cannot keep honest"
      )
    elsif [match[1].to_i, match[2].to_i] != [crates, copies]
      findings << Finding.new(
        file: DENY, rule: "prose-counts-derived",
        detail: "the header says #{match[1]} crates / #{match[2]} copies; the skip list derives #{crates} / #{copies}",
        fix: "update the prose to the derived numbers — the skip list is the fact, the sentence is the copy"
      )
    end

    lock, error = read_toml(root, LOCK)
    if lock.nil?
      findings << Finding.new(file: LOCK, rule: "prose-counts-derived", detail: error,
                              fix: "the external-package count is derived from the lock")
      return
    end

    external = (lock.arrays["package"] || []).count { |p| p.key?("source") }
    match = text.match(EXT_PHRASE)
    if match.nil?
      findings << Finding.new(
        file: DENY, rule: "prose-counts-derived",
        detail: "the licenses note no longer states the external-package count (expected the phrase 'N external packages')",
        fix: "restore the sentence; a count this rule cannot find is a count it cannot keep honest"
      )
    elsif match[1].to_i != external
      findings << Finding.new(
        file: DENY, rule: "prose-counts-derived",
        detail: "the licenses note says #{match[1]} external packages; the lock records #{external} (entries carrying a `source`)",
        fix: "update the prose to the derived number"
      )
    end
  end

  # Dependency-update coverage is a set that must equal another set: every
  # package.json tree in the repository needs an npm entry in dependabot.yml,
  # and every npm entry needs a tree. The required set is walked, never listed —
  # npm coverage was removed wholesale at 3446b652 with nothing noticing for
  # nine days, and a hand-kept list is how the next removal also goes silent
  # (§4.4 shape 2, both halves: the stale list and the stale entry).
  def check_dependabot_coverage(root, findings)
    path = root.join(DEPENDABOT)
    unless path.file?
      findings << Finding.new(
        file: DEPENDABOT, rule: "dependabot-npm-coverage",
        detail: "#{DEPENDABOT} does not exist",
        fix: "without it no ecosystem receives updates, which is a policy decision nobody recorded"
      )
      return
    end

    begin
      config = YAML.safe_load(path.read, aliases: true)
    rescue Psych::Exception => e
      findings << Finding.new(
        file: DEPENDABOT, rule: "dependabot-npm-coverage",
        detail: "#{DEPENDABOT} could not be parsed: #{e.message}",
        fix: "a config Dependabot cannot read updates nothing while looking like coverage"
      )
      return
    end

    updates = config.is_a?(Hash) ? config["updates"] : nil
    unless updates.is_a?(Array) && !updates.empty?
      findings << Finding.new(
        file: DEPENDABOT, rule: "dependabot-npm-coverage",
        detail: "#{DEPENDABOT} has no updates entries",
        fix: "declare the ecosystems; an empty config is the 3446b652 state with extra steps"
      )
      return
    end

    ecosystems = updates.map { |u| u.is_a?(Hash) ? u["package-ecosystem"].to_s : "" }
    %w[cargo github-actions].each do |ecosystem|
      next if ecosystems.include?(ecosystem)

      findings << Finding.new(
        file: DEPENDABOT, rule: "dependabot-npm-coverage",
        detail: "no #{ecosystem} entry in #{DEPENDABOT}",
        fix: "the #{ecosystem} tree exists whether or not anything watches it"
      )
    end

    declared = updates.select { |u| u.is_a?(Hash) && u["package-ecosystem"] == "npm" }
                      .map { |u| u["directory"].to_s.delete_prefix("/").chomp("/") }
    derived = npm_trees(root)

    if derived.empty?
      findings << Finding.new(
        file: DEPENDABOT, rule: "empty-scan",
        detail: "the tree walk found no package.json, so npm coverage examined nothing",
        fix: "this repository has npm trees; a walk that finds none is a broken walk, not a covered repo"
      )
      return
    end

    (derived - declared).each do |tree|
      findings << Finding.new(
        file: DEPENDABOT, rule: "dependabot-npm-coverage",
        detail: "#{tree}/package.json has no npm entry in #{DEPENDABOT}",
        fix: "add the entry, or record why this tree is exempt — silence is how the last removal went"
      )
    end

    (declared - derived).each do |dir|
      findings << Finding.new(
        file: DEPENDABOT, rule: "dependabot-npm-coverage",
        detail: "the npm entry for /#{dir} points at no package.json",
        fix: "delete the entry; it covers nothing and hides the next tree to take that path"
      )
    end
  end

  # Relative directories of every package.json outside pruned dirs, sorted.
  def npm_trees(root)
    trees = []
    Find.find(root.to_s) do |path|
      base = File.basename(path)
      if File.directory?(path)
        Find.prune if PRUNED_DIRS.include?(base)
        next
      end
      trees << Pathname.new(path).dirname.relative_path_from(root).to_s if base == "package.json"
    end
    trees.sort
  end

  def run(root)
    findings = []
    check_workflow(root, findings)
    doc = check_policy(root, findings)
    check_skips_live(root, doc, findings)
    check_audit(root, findings)
    check_prose_counts(root, doc, findings)
    check_dependabot_coverage(root, findings)
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
