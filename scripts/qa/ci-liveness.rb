#!/usr/bin/env ruby
#
# FR-134 requirement 8: CI liveness.
#
# The enforcement surface ledger classifies scripts/qa/*. That is a strictly
# smaller thing than the CI it runs on: `boundary-coverage`, `test`, `clippy`,
# `miri` and `cross-compile` are not in it and never were. A liveness check
# scoped to the gates it already knows about would not have looked at
# boundary-coverage once, and boundary-coverage had been red six runs running.
#
# So the object list is discovered by parsing the workflows, and the ledger is
# only allowed to say what the outcome was. A job that appears in a workflow and
# not in the ledger fails. A job recorded as failing without a written
# `knownFailing` reference fails. And a record whose workflow has changed since
# it was taken is stale, which is what stops this from decaying into the same
# unreviewed declaration everything else in this FR was filed about.
#
# Verification is offline: it reads the ledger, the workflows and git. Only
# --refresh talks to GitHub, and like every other governance writer here it
# refuses to run unattended.
#
# Usage:
#   ci-liveness.rb                 verify the ledger against the workflows
#   ci-liveness.rb --emit          print the ledger a refresh would produce
#   ci-liveness.rb --refresh       pull real outcomes from `gh run` (local only)
#   ci-liveness.rb --refresh --write   apply them

require "json"
require "optparse"
require "open3"
require "pathname"
require_relative "../lib/workflow_model"
require_relative "../lib/ci_env"

REPO_ROOT = Pathname.new(__dir__).join("..", "..").cleanpath
LEDGER_REL = "config/governance/ci-job-liveness.json"
WORKFLOW_GLOB = ".github/workflows/*.yml"

options = { refresh: false, write: false, emit: false, branch: "main" }
OptionParser.new do |parser|
  parser.on("--refresh") { options[:refresh] = true }
  parser.on("--write") { options[:write] = true }
  parser.on("--emit") { options[:emit] = true }
  parser.on("--branch BRANCH") { |value| options[:branch] = value }
end.parse!

ledger_path = REPO_ROOT.join(LEDGER_REL)
unless ledger_path.file?
  warn "missing ledger: #{LEDGER_REL}"
  exit 1
end
ledger = JSON.parse(File.read(ledger_path))

# Every workflow on disk. Discovery, not enumeration: a new workflow file is in
# scope the moment it lands, and has to be either recorded or excused.
def discovered_workflows
  Dir[REPO_ROOT.join(WORKFLOW_GLOB).to_s].sort.map do |path|
    Pathname.new(path).relative_path_from(REPO_ROOT).to_s
  end
end

def git(*args)
  stdout, _stderr, status = Open3.capture3("git", "-C", REPO_ROOT.to_s, *args)
  [stdout.strip, status.success?]
end

# Has the workflow changed since the run we recorded? If so the recorded
# outcome describes a pipeline that no longer exists.
def workflow_changed_since?(sha, workflow)
  log, ok = git("log", "--oneline", "#{sha}..HEAD", "--", workflow)
  return false unless ok

  !log.empty?
end

def ancestor?(sha)
  _out, ok = git("merge-base", "--is-ancestor", sha, "HEAD")
  ok
end

errors = []
entries = ledger["workflows"] || []
by_path = entries.each_with_object({}) { |entry, acc| acc[entry["path"]] = entry }

# 1. Coverage in both directions.
discovered = discovered_workflows
(discovered - by_path.keys).each do |missing|
  errors << "#{missing} is a workflow on disk with no entry in the ledger; " \
            "record its jobs or exclude it with a reason"
end
(by_path.keys - discovered).each do |extra|
  errors << "#{extra} is recorded in the ledger but is not a workflow on disk"
end

entries.each do |entry|
  path = entry["path"]
  next unless discovered.include?(path)

  unless entry["inScope"]
    reason = entry["reason"].to_s
    errors << "#{path} is excluded from liveness with no reason" if reason.empty?
    next
  end

  jobs = entry["jobs"] || {}
  actual = WorkflowModel.jobs(REPO_ROOT.join(path).to_s)

  # 2. Every job in the workflow is recorded, and every record names a real job.
  (actual - jobs.keys).each do |missing|
    errors << "#{path}: job '#{missing}' has no liveness record; " \
              "adding a job to a workflow requires recording how it does"
  end
  (jobs.keys - actual).each do |extra|
    errors << "#{path}: liveness record for '#{extra}', which the workflow no longer defines"
  end

  jobs.each do |job, record|
    next unless actual.include?(job)

    conclusion = record["conclusion"].to_s
    known = record["knownFailing"]

    # 3. A red job needs an owner and a reason, in writing.
    if conclusion != "success"
      if known.nil?
        errors << "#{path}: job '#{job}' last concluded '#{conclusion}' and is not marked " \
                  "known-failing; fix it or record the reference and reason"
      else
        %w[reference reason].each do |field|
          if known[field].to_s.empty?
            errors << "#{path}: job '#{job}' is known-failing without a #{field}"
          end
        end
      end
    elsif known
      errors << "#{path}: job '#{job}' is marked known-failing but last concluded success; " \
                "remove the annotation so the next real failure is visible"
    end

    # 4. Freshness. A record taken before the workflow last changed describes a
    #    pipeline that no longer exists, and is worth less than no record.
    sha = record["headSha"].to_s
    if sha.empty?
      errors << "#{path}: job '#{job}' has no headSha, so the record cannot be dated"
      next
    end
    unless ancestor?(sha)
      errors << "#{path}: job '#{job}' records #{sha[0, 8]}, which is not an ancestor of HEAD; " \
                "refresh against a run from this history"
      next
    end
    if workflow_changed_since?(sha, path)
      errors << "#{path}: job '#{job}' was recorded at #{sha[0, 8]}, before #{path} last changed; " \
                "the record describes a pipeline that no longer exists — re-run and refresh"
    end
  end
end

# ── Refresh ──────────────────────────────────────────────────────────────────

def gh_json(*args)
  stdout, stderr, status = Open3.capture3("gh", *args)
  unless status.success?
    warn "gh #{args.join(' ')} failed: #{stderr.strip}"
    exit 1
  end
  JSON.parse(stdout)
end

# gh reports a matrix job as "Base name (leg)". Map back to the workflow's job
# key through its `name:`, falling back to the key itself, and take the worst
# leg — a matrix job is green only when every leg is.
def match_job(workflow_path, key, gh_jobs)
  definition = WorkflowModel.job(workflow_path, key) || {}
  template = definition["name"] || key
  # Everything before the first expression is the stable part of the rendered
  # name. Taking the first word instead turns "Coverage policy fixtures
  # (${{ matrix.os }})" into "Coverage", which matches nothing and quietly
  # records the job as never having run.
  base = template.split("${{").first.to_s.sub(/[\s(]+\z/, "").strip
  base = key if base.empty?
  gh_jobs.select { |job| job["name"] == base || job["name"].start_with?("#{base} (") }
end

WORST = %w[success skipped neutral cancelled timed_out action_required failure].freeze

if options[:refresh] || options[:emit]
  CiEnv.refuse_unattended_write!(
    "CI liveness ledger",
    "run --refresh locally, read the diff, and commit it with the change that fixed the job"
  ) if options[:write]

  refreshed = { "version" => ledger["version"], "description" => ledger["description"], "workflows" => [] }
  entries.each do |entry|
    path = entry["path"]
    unless entry["inScope"] && discovered.include?(path)
      refreshed["workflows"] << entry
      next
    end

    runs = gh_json("run", "list", "--workflow=#{File.basename(path)}",
                   "--branch=#{options[:branch]}", "--limit", "20",
                   "--json", "databaseId,headSha,conclusion,status")
    run = runs.find { |candidate| candidate["status"] == "completed" && candidate["conclusion"] != "cancelled" }
    unless run
      warn "#{path}: no completed run found on #{options[:branch]}"
      refreshed["workflows"] << entry
      next
    end

    detail = gh_json("run", "view", run["databaseId"].to_s, "--json", "jobs")
    gh_jobs = detail["jobs"] || []
    workflow_path = REPO_ROOT.join(path).to_s
    jobs = {}
    WorkflowModel.jobs(workflow_path).each do |key|
      legs = match_job(workflow_path, key, gh_jobs)
      conclusion =
        if legs.empty?
          "not-run"
        else
          legs.map { |leg| leg["conclusion"].to_s }.max_by { |value| WORST.index(value) || WORST.length }
        end
      previous = (entry["jobs"] || {})[key] || {}
      record = {
        "conclusion" => conclusion,
        "runId" => run["databaseId"].to_s,
        "headSha" => run["headSha"].to_s
      }
      # A knownFailing annotation is a human judgement; refreshing outcomes must
      # not silently drop it, and must not invent one either.
      record["knownFailing"] = previous["knownFailing"] if previous["knownFailing"] && conclusion != "success"
      jobs[key] = record
    end
    refreshed["workflows"] << entry.merge("jobs" => jobs)
  end

  serialised = "#{JSON.pretty_generate(refreshed)}\n"
  if options[:write]
    File.write(ledger_path, serialised)
    warn "wrote #{LEDGER_REL}; read the diff and commit it"
  else
    puts serialised
  end
  exit 0
end

# ── Verify ───────────────────────────────────────────────────────────────────

if errors.empty?
  recorded = entries.select { |entry| entry["inScope"] }.sum { |entry| (entry["jobs"] || {}).length }
  red = entries.flat_map { |entry| ((entry["jobs"] || {}).values) }
    .count { |record| record["conclusion"] != "success" }
  puts "CI liveness: PASS"
  puts "  #{recorded} job(s) recorded across #{entries.count { |e| e['inScope'] }} in-scope workflow(s); " \
       "#{red} known-failing"
  exit 0
end

warn "CI liveness: FAIL"
errors.each { |error| warn "  #{error}" }
exit 1
