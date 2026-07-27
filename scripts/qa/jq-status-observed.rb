#!/usr/bin/env ruby
# frozen_string_literal: true

# FR-144: a gate must not be able to stop checking while reporting PASS.
#
# The defect this guards against, in the shape it actually occurred: a manifest
# entry was written as `"providerIsolation": "no-provider"` where the schema
# requires `{"mode": "no-provider"}`. jq exited 5. The loop being fed by
#
#   done < <(jq -r '<query>' "$manifest")
#
# read zero rows, its body never ran, the check returned 0, and
# test-qa-gate-surface.sh printed "13 passed, 0 failed" over a manifest it could
# not read. Nobody looks at a process substitution's exit status, and
# `set -euo pipefail` does not change that.
#
# There is a second channel with the same silence. Every check is invoked as
# `"$check" "$root" || return 1` — condition position — which disables `set -e`
# for the whole call tree beneath it, so an unchecked capture is equally quiet.
#
# Without this scanner the shape returns with the next gate anyone writes. The
# five gates were converted by hand once; that fixes the past, not the future.
#
# The scanned set is derived, never listed: every ci-required gate in
# config/governance/qa-gate-surface.json that is a shell script, plus the shared
# libraries under scripts/lib. A gate registered tomorrow is in scope tomorrow.
#
# Comments and here-document bodies are removed by scripts/lib/shell_lexer.rb
# before any matching, because a gate that documents the forbidden pattern in
# prose — this repository's design records do exactly that — must not be flagged
# for describing it. A grep would be a proxy for the fact under test, which is
# the error FR-144 exists to correct.

require "json"
require "open3"
require "optparse"
require "pathname"

require_relative "../lib/shell_lexer"

module JqStatusObserved
  MANIFEST_REL = "config/governance/qa-gate-surface.json"
  READER = "gate_jq_rows"

  Finding = Struct.new(:file, :line, :rule, :detail, :fix, keyword_init: true)

  module_function

  # ── Scope ───────────────────────────────────────────────────────────────────

  # ci-required shell gates, from the manifest, plus the shared libraries they
  # source. Discovery rather than enumeration: a hand list guards exactly what
  # was known the day it was written, which is the failure this repository has
  # removed from a dozen other checks.
  def in_scope(repo_root)
    manifest = JSON.parse(File.read(repo_root.join(MANIFEST_REL)))
    gates = manifest.fetch("scripts", [])
                    .select { |entry| entry["enforcement"] == "ci-required" }
                    .map { |entry| entry["path"] }
                    .select { |path| path.to_s.end_with?(".sh") }

    libraries = tracked(repo_root, "scripts/lib/*.sh")

    (gates + libraries).uniq.select { |path| repo_root.join(path).file? }.sort
  end

  def tracked(repo_root, pattern)
    output, status = Open3.capture2("git", "-C", repo_root.to_s, "ls-files", "-z", pattern)
    raise "git ls-files failed for #{pattern}" unless status.success?

    output.split("\0").reject(&:empty?)
  end

  # ── Reachability ────────────────────────────────────────────────────────────

  # Function names defined in this file whose body can reach jq — directly, or
  # through the shared reader, or through another function in the same file.
  #
  # Transitive because the count that matters is not textual. FR-144 was filed
  # counting `done < <(jq`, found 17 sites, and missed 22 more where the feed was
  # a function that runs jq one call deeper. test-docs-publishing-integrity.sh
  # has exactly one direct occurrence and twenty-two real ones.
  def jq_reaching_functions(code)
    bodies = function_bodies(code)
    reaching = {}

    # Fixed point over the truthy set only. Counting every visited key instead
    # would treat "asked and answered no" as "reaches jq", which flagged
    # extract_links (awk) and bundle_providers (ruby) on the first run — the
    # scanner reporting a defect that was not there, which is worse than the one
    # it was written to catch.
    loop do
      before = reaching.size
      bodies.each do |name, body|
        next if reaching[name]

        direct = body.match?(/(?<![\w-])(?:jq|#{READER})(?![\w-])/)
        indirect = reaching.keys.any? { |other| body.match?(/(?<![\w-])#{Regexp.escape(other)}(?![\w-])/) }
        reaching[name] = true if direct || indirect
      end
      break if reaching.size == before
    end

    reaching
  end

  # Crude but adequate: a definition line opens the body, and a `}` at column
  # zero closes it. Every shell file in this repository is written that way, and
  # a missed body can only cause a false negative in the reachability map, never
  # a false positive on a `done < <(jq …)` line, which is matched directly.
  def function_bodies(code)
    bodies = {}
    current = nil
    buffer = []

    code.each do |_number, line|
      if (match = line.match(/^([a-z_][a-z0-9_]*)\s*\(\)\s*\{/))
        bodies[current] = buffer.join("\n") if current
        current = match[1]
        buffer = [line]
      elsif current
        buffer << line
        if line.start_with?("}")
          bodies[current] = buffer.join("\n")
          current = nil
          buffer = []
        end
      end
    end
    bodies[current] = buffer.join("\n") if current
    bodies
  end

  # ── Rules ───────────────────────────────────────────────────────────────────

  # A file that keeps the process-wide failure record and asserts on it has an
  # answer for feeds this scanner cannot follow — a read four loops deep inside
  # nested process substitutions, where a non-zero return has nowhere to go.
  # Such a file is allowed to feed a loop from a jq-reaching function.
  #
  # It is not allowed to write `done < <(jq …)` directly: that form is always
  # convertible at the call site, and leaving it is a choice to be silent.
  def maintains_failure_record?(code)
    text = code.map(&:last).join("\n")
    text.include?("gate_jq_begin") && text.include?("gate_jq_failure_count")
  end

  def scan_file(repo_root, relative)
    text = repo_root.join(relative).read
    code, state = ShellLexer.code_lines(text)
    findings = []

    if state.in_heredoc?
      findings << Finding.new(
        file: relative, line: state.heredoc_line, rule: "unclosed-heredoc",
        detail: "this file ends inside a here-document, so everything after this line went unscanned",
        fix: "close the here-document; an unscanned region cannot be certified"
      )
      return findings
    end

    reaching = jq_reaching_functions(code)
    recorded = maintains_failure_record?(code)

    code.each do |number, line|
      if (match = line.match(/done\s*<\s*<\(\s*([^\s)]+)/))
        feed = match[1]

        if feed == "jq"
          findings << Finding.new(
            file: relative, line: number, rule: "unobserved-feed",
            detail: "a loop is fed by jq through a process substitution, whose exit status nobody reads",
            fix: "rows=\"$(#{READER} <require-rows|allow-empty> \"$file\" '<query>')\" || return 1, then: done <<< \"$rows\""
          )
        elsif reaching[feed] && !recorded
          findings << Finding.new(
            file: relative, line: number, rule: "unrecorded-feed",
            detail: "a loop is fed by #{feed}, which can reach jq, and this file keeps no failure record",
            fix: "capture and test the feed, or call gate_jq_begin and assert on gate_jq_failure_count"
          )
        end
      end

      # jq's status is replaced by the last stage of the pipeline, so this is
      # unobservable regardless of what the caller does with the result. It is
      # what check_surface_complete did: `declared="$(jq -r … | LC_ALL=C sort)"`.
      if line =~ /\$\(\s*jq\b[^)]*\|/
        findings << Finding.new(
          file: relative, line: number, rule: "status-dropped-by-pipe",
          detail: "jq is piped inside a command substitution, so the substitution reports the last stage's status, not jq's",
          fix: "read with #{READER} first, then pipe the captured rows"
        )
      end
    end

    findings
  end

  def run(repo_root)
    files = in_scope(repo_root)
    findings = files.flat_map { |relative| scan_file(repo_root, relative) }
    [files, findings]
  end
end

if $PROGRAM_NAME == __FILE__
  repo_root = Pathname.new(File.expand_path("../..", __dir__))
  list_only = false

  OptionParser.new do |opts|
    opts.banner = "usage: jq-status-observed.rb [--list-files] [--repo-root PATH]"
    opts.on("--list-files", "print the scanned set and exit") { list_only = true }
    opts.on("--repo-root PATH", "scan a different checkout") { |value| repo_root = Pathname.new(value) }
  end.parse!(ARGV)

  if list_only
    puts JqStatusObserved.in_scope(repo_root)
    exit 0
  end

  files, findings = JqStatusObserved.run(repo_root)

  findings.each do |finding|
    warn "#{finding.file}:#{finding.line}: [#{finding.rule}] #{finding.detail}"
    warn "  fix: #{finding.fix}"
  end

  if findings.empty?
    puts "jq status observed: PASS (#{files.length} shell gate(s) scanned, 0 finding(s))"
    exit 0
  end

  warn ""
  warn "jq status observed: FAIL (#{files.length} scanned, #{findings.length} finding(s))"
  exit 1
end
