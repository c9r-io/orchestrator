#!/usr/bin/env ruby
#
# FR-140 requirements 1 and 3: what each governance gate costs, and a ceiling on
# the total.
#
# Gate correctness has been governed continuously since FR-127. Gate *cost* had
# never been recorded, so "add one more gate" was a zero-cost decision every
# time — and the surface went from 45 entries to 65 in three days while the two
# governance jobs grew from 45 minutes to 68. Nothing in the repository could
# have said which gate the minutes went to, because `ci-job-liveness.json`
# records each job's last conclusion and `qa-gate-surface.json` records each
# gate's classification, and neither records a duration.
#
# The object list is discovered by reading the workflow and the enforcement
# surface together: a step is in scope when it executes a `ci-required` gate.
# Both ledgers have to agree, which is what makes "a gate whose step is not
# recorded" a failure rather than an omission nobody sees. Enumerating the
# steps here would guard exactly the steps that existed the day it was written.
#
# ── Why this ledger compares against a threshold and `sourceBaseline` does not
#
# FR-128 tightened `coordination-governance.rb`'s `sourceBaseline` from
# monotonic to exact equality, and that was right: a reference count is a
# deterministic function of the tree, so a count that moved without a reviewed
# ledger update is a defect by definition.
#
# A duration is not that. It is a sample from a distribution — runner hardware,
# cache state, network, the other tenants on the box. Measured here across six
# successful runs at a fixed gate count, the two governance jobs vary by about
# ±7% run to run. Exact equality on a random variable is a gate that fails on
# noise, and a gate that fails on noise gets a `knownFailing` annotation and
# then gets ignored, which is worse than not having it. So the recorded numbers
# are attribution, and the only thing that can fail is the budget.
#
# Verification is offline: the ledger, the workflow, the enforcement surface and
# git. Only --refresh talks to GitHub, and like every other governance writer
# here it refuses to run unattended.
#
# Usage:
#   ci-cost.rb                      verify the ledger and the budget
#   ci-cost.rb --emit               print the ledger a refresh would produce
#   ci-cost.rb --refresh            pull real step timings from `gh run` (local only)
#   ci-cost.rb --refresh --write    apply them

require "json"
require "set"
require "optparse"
require "open3"
require "pathname"
require_relative "../lib/workflow_model"
require_relative "../lib/ci_env"

REPO_ROOT = Pathname.new(__dir__).join("..", "..").cleanpath
LEDGER_REL = "config/governance/ci-step-cost.json"
SURFACE_REL = "config/governance/qa-gate-surface.json"

options = { refresh: false, write: false, emit: false, branch: "main" }
OptionParser.new do |parser|
  parser.on("--refresh") { options[:refresh] = true }
  parser.on("--write") { options[:write] = true }
  parser.on("--emit") { options[:emit] = true }
  parser.on("--branch BRANCH") { |value| options[:branch] = value }
end.parse!

def read_json(relative)
  path = REPO_ROOT.join(relative)
  unless path.file?
    warn "missing: #{relative}"
    exit 1
  end
  JSON.parse(File.read(path))
end

ledger = read_json(LEDGER_REL)
surface = read_json(SURFACE_REL)
workflow_rel = ledger["workflow"]
workflow_path = REPO_ROOT.join(workflow_rel).to_s

def git(*args)
  stdout, _stderr, status = Open3.capture3("git", "-C", REPO_ROOT.to_s, *args)
  [stdout.strip, status.success?]
end

def ancestor?(sha)
  _out, ok = git("merge-base", "--is-ancestor", sha, "HEAD")
  ok
end

# ── Critical path ────────────────────────────────────────────────────────────
#
# FR-174 requirement 4. Everything above this line is per-job and per-step
# seconds, and the number a developer actually waits for is neither: the jobs
# run in parallel, so feedback latency is the longest chain through the `needs`
# graph, not the sum. That distinction is the whole of FR-174's argument and it
# was left for each reader to re-derive from the workflow — which is how the
# FR's own acceptance criterion came to compare a post-tiering critical path
# against the *sum* of five parallel jobs, a quantity that bounds nothing.
# A number an argument rests on belongs in the file the argument cites.
#
# Derived on every run and compared, never stored and trusted: a recorded
# latency that nobody recomputes is a duration pinned to a graph that has since
# changed, which is the failure `pendingMeasurement` exists to prevent one level
# down.
def job_needs(workflow_path, key)
  definition = WorkflowModel.job(workflow_path, key) || {}
  Array(definition["needs"]).compact
end

# Longest chain by seconds. Memoised over the DAG rather than enumerated,
# because `needs` is a graph and today's single edge is a fact about the
# workflow now, not about the shape of the answer.
def longest_chain(key, jobs, workflow_path, seen = {})
  return seen[key] if seen.key?(key)

  seconds = jobs.dig(key, "seconds").to_i
  best = { "seconds" => seconds, "chain" => [key] }
  job_needs(workflow_path, key).each do |dependency|
    next unless jobs.key?(dependency)

    upstream = longest_chain(dependency, jobs, workflow_path, seen)
    candidate = upstream["seconds"] + seconds
    best = { "seconds" => candidate, "chain" => upstream["chain"] + [key] } if candidate > best["seconds"]
  end
  seen[key] = best
  best
end

# The steps `ci.yml` runs conditionally on the meta-verification tier, and what
# they cost. Derived from the workflow's `if:` expressions, so a gate added to
# or removed from the tier moves this number without anyone editing it.
def tiered_steps(workflow_path, job_key)
  WorkflowModel.steps(workflow_path, job_key)
    .select { |step| step["if"].to_s.include?("tier.outputs.tier") }
    .map { |step| step["name"].to_s }
end

def critical_path(jobs, workflow_path)
  return nil if jobs.empty?

  full = jobs.keys.map { |key| longest_chain(key, jobs, workflow_path) }
    .max_by { |entry| entry["seconds"] }

  # The deferred tier is the same graph with the tier-conditional steps removed
  # from the job that carries them. Their seconds are subtracted from that job's
  # total rather than from the step map, because the budget and the latency are
  # both stated against the total.
  deferred_jobs = jobs.transform_values { |record| record.dup }
  tiered_total = 0
  tiered_count = 0
  deferred_jobs.each_key do |key|
    names = tiered_steps(workflow_path, key)
    next if names.empty?

    measured = (deferred_jobs[key]["steps"] || {}).select { |name, _| names.include?(name) }
    tiered_total += measured.values.sum
    tiered_count += names.length
    deferred_jobs[key] = deferred_jobs[key].merge("seconds" => deferred_jobs[key]["seconds"] - measured.values.sum)
  end

  deferred = deferred_jobs.keys.map { |key| longest_chain(key, deferred_jobs, workflow_path, {}) }
    .max_by { |entry| entry["seconds"] }

  {
    "description" =>
      "Feedback latency: the longest chain through the `needs` graph, which is what a " \
      "developer waits for. Not the sum of the jobs — they run in parallel — and not any " \
      "single job's seconds. Derived from this file's per-job totals and ci.yml's `needs` " \
      "edges on every run of ci-cost.rb, and compared against what is recorded here, so it " \
      "cannot describe a graph the workflow no longer has. `deferred` is the same graph with " \
      "the meta-verification steps FR-174 made tier-conditional subtracted from the job that " \
      "carries them; it is what a pull request touching no gate root waits for, and `full` is " \
      "what one touching scripts/qa, scripts/lib, config/governance or .github/workflows waits " \
      "for. Compare either against the longest *product* job, never against the product jobs' " \
      "sum.",
    "full" => full,
    "deferred" => deferred,
    "tieredSteps" => tiered_count,
    "tieredSeconds" => tiered_total
  }
end

# The steps that execute a given gate, by name, within its declared job. Uses
# the same executable-text predicate the enforcement surface gate uses for
# wiring truth (`WorkflowModel.executes?`), so a commented-out `run:`, an
# `if: false` step, a `name:` mention and a heredoc body all fail to count here
# exactly as they fail to count there. A gate declared with `invokedBy` is
# looked up through its invoker, because that is the step GitHub bills.
def steps_running(path, job, gate, invoked_by)
  target = "./#{invoked_by || gate}"
  WorkflowModel.steps(path, job).reject { |step| WorkflowModel.disabled?(step) }.select do |step|
    text = WorkflowModel.executable_shell(step["run"])
    text.match?(/(?:^|[\s;&|(`"'])#{Regexp.escape(target)}(?:$|[\s;&|)`"'])/)
  end.map { |step| step["name"].to_s }.reject(&:empty?)
end

# Every ci-required gate that this workflow is supposed to run, paired with the
# step that runs it. Discovery from two ledgers at once: the enforcement surface
# says which gates must be enforced, the workflow says where.
def required_steps(workflow_rel, workflow_path, surface)
  wanted = {}
  missing = []
  surface["scripts"].each do |entry|
    next unless entry["enforcement"] == "ci-required"
    next unless entry["workflow"] == workflow_rel

    job = entry["job"].to_s
    names = steps_running(workflow_path, job, entry["path"], entry["invokedBy"])
    if names.empty?
      missing << "#{entry['path']}: no step in job '#{job}' of #{workflow_rel} executes it"
      next
    end
    names.each { |name| (wanted[job] ||= {})[name] = (wanted[job][name] || []) + [entry["path"]] }
  end
  [wanted, missing]
end

errors = []
recorded_jobs = ledger["jobs"] || {}
wanted, unwired = required_steps(workflow_rel, workflow_path, surface)
errors.concat(unwired)

# ── Refresh ──────────────────────────────────────────────────────────────────

def gh_json(*args)
  stdout, stderr, status = Open3.capture3("gh", *args)
  unless status.success?
    warn "gh #{args.join(' ')} failed: #{stderr.strip}"
    exit 1
  end
  JSON.parse(stdout)
end

def seconds_between(started, completed)
  return nil if started.to_s.empty? || completed.to_s.empty?

  (Time.parse(completed) - Time.parse(started)).round
end

# gh reports a matrix job as "Base name (leg)", the same shape ci-liveness.rb
# resolves. Cost takes the slowest leg rather than the sum: the job's
# contribution to wall clock is when its last leg finishes, and legs run
# concurrently.
def matching_legs(workflow_path, key, gh_jobs)
  definition = WorkflowModel.job(workflow_path, key) || {}
  template = definition["name"] || key
  base = template.split("${{").first.to_s.sub(/[\s(]+\z/, "").strip
  base = key if base.empty?
  gh_jobs.select { |job| job["name"] == base || job["name"].start_with?("#{base} (") }
end

if options[:refresh] || options[:emit]
  require "time"

  if options[:write]
    CiEnv.refuse_unattended_write!(
      "CI step cost ledger",
      "run --refresh locally, read the diff, and commit it with the change that moved the cost"
    )
  end

  runs = gh_json("run", "list", "--workflow=#{File.basename(workflow_rel)}",
                 "--branch=#{options[:branch]}", "--limit", "20",
                 "--json", "databaseId,headSha,conclusion,status")
  run = runs.find { |candidate| candidate["status"] == "completed" && candidate["conclusion"] != "cancelled" }
  unless run
    warn "no completed run found on #{options[:branch]}"
    exit 1
  end

  detail = gh_json("run", "view", run["databaseId"].to_s, "--json", "jobs")
  gh_jobs = detail["jobs"] || []

  jobs = {}
  WorkflowModel.jobs(workflow_path).each do |key|
    legs = matching_legs(workflow_path, key, gh_jobs)
    next if legs.empty?

    slowest = legs.max_by { |leg| seconds_between(leg["startedAt"], leg["completedAt"]) || 0 }
    total = seconds_between(slowest["startedAt"], slowest["completedAt"])
    next if total.nil?

    # Only the steps the workflow defines. GitHub injects `Set up job`,
    # `Post <name>` and `Complete job` around them; recording those as if the
    # repository had written them would put names in the ledger that no edit
    # here can ever change.
    defined_steps = WorkflowModel.steps(workflow_path, key).map { |step| step["name"].to_s }.to_set
    steps = {}
    (slowest["steps"] || []).each do |step|
      name = step["name"].to_s
      next unless defined_steps.include?(name)

      value = seconds_between(step["startedAt"], step["completedAt"])
      steps[name] = value unless value.nil?
    end
    # Setup, caching and teardown are real seconds that no step in this file
    # accounts for. Stated rather than dropped: a breakdown whose parts do not
    # reach the total reads like a full accounting and is not one, and the
    # budget is enforced on the total.
    jobs[key] = {
      "seconds" => total,
      "unattributed" => total - steps.values.sum,
      "steps" => steps
    }
  end

  # A pending acknowledgement is a human note about a step nobody has a number
  # for yet. Refreshing must not invent one and must not silently keep one that
  # the run just measured — the annotation drops itself the moment it is false.
  measured = jobs.values.flat_map { |record| (record["steps"] || {}).keys }.to_set
  still_pending = (ledger["pendingMeasurement"] || {}).reject { |name, _| measured.include?(name) }

  refreshed = {
    "version" => ledger["version"],
    "description" => ledger["description"],
    "workflow" => workflow_rel,
    "budget" => ledger["budget"],
    "pendingMeasurement" => still_pending,
    "measurement" => { "runId" => run["databaseId"].to_s, "headSha" => run["headSha"].to_s },
    "criticalPath" => critical_path(jobs, workflow_path),
    "jobs" => jobs
  }

  serialised = "#{JSON.pretty_generate(refreshed)}\n"
  if options[:write]
    File.write(REPO_ROOT.join(LEDGER_REL), serialised)
    warn "wrote #{LEDGER_REL}; read the diff and commit it"
  else
    puts serialised
  end
  exit 0
end

# ── Verify ───────────────────────────────────────────────────────────────────

# 1. Every ci-required gate's step is recorded, and every record names a step the
#    workflow still defines. A ledger that only grew when someone remembered is
#    the enumeration failure this gate exists to prevent, so the check runs in
#    both directions.
#
#    A step that was added since the measurement has no cost yet and cannot
#    have one until CI runs it. That window is acknowledged explicitly rather
#    than waved through, because "what does one more gate cost?" is the question
#    this ledger exists to make someone answer. The acknowledgement names the
#    step and says why, and the next refresh replaces it with a number.
pending = ledger["pendingMeasurement"] || {}
wanted.each do |job, steps|
  recorded = (recorded_jobs[job] || {})["steps"] || {}
  steps.each do |name, gates|
    next if recorded.key?(name)

    if (note = pending[name])
      errors << "#{job}: step '#{name}' is pending measurement without a reason" if note.to_s.empty?
      next
    end
    errors << "#{job}: step '#{name}' has no cost record; it runs #{gates.sort.join(', ')}. " \
              "Refresh against a run that includes it, or record it under pendingMeasurement " \
              "with the reason it cannot be measured yet"
  end
end

pending.each_key do |name|
  next unless recorded_jobs.any? { |_job, record| (record["steps"] || {}).key?(name) }

  errors << "step '#{name}' is marked pending measurement but has a recorded cost; " \
            "drop the annotation so the next unmeasured step is visible"
end

recorded_jobs.each do |job, record|
  defined_steps = WorkflowModel.steps(workflow_path, job).map { |step| step["name"].to_s }
  unless WorkflowModel.jobs(workflow_path).include?(job)
    errors << "cost record for job '#{job}', which #{workflow_rel} no longer defines"
    next
  end
  ((record["steps"] || {}).keys - defined_steps).each do |extra|
    errors << "#{job}: cost record for step '#{extra}', which the job no longer defines"
  end
end

# 2. Provenance. The measurement has to come from a run on this history, or the
#    numbers describe someone else's pipeline.
#
#    Deliberately *not* ci-liveness.rb's "has the workflow changed since?" rule.
#    That rule is right for a conclusion, which any edit can invalidate, and
#    wrong here: bumping a `runs-on` or fixing a comment does not make a recorded
#    duration a lie, and a rule that says it does trains everyone to refresh
#    without reading. What actually invalidates a cost record is the step set
#    moving, and check 1 observes that directly in both directions — which is
#    also what makes a newly added gate ask for its own number instead of
#    hiding inside a stale total.
measurement = ledger["measurement"] || {}
sha = measurement["headSha"].to_s
if sha.empty?
  errors << "the measurement has no headSha, so it cannot be dated"
elsif !ancestor?(sha)
  errors << "the measurement records #{sha[0, 8]}, which is not an ancestor of HEAD; " \
            "refresh against a run from this history"
end

# 2b. The critical path, recomputed rather than read. The recorded value is a
#     function of this file's per-job seconds and ci.yml's `needs` edges, so it
#     can be re-derived exactly — and a stored latency nobody re-derives is a
#     duration pinned to a graph that has since changed. Adding one `needs` edge
#     would silently invalidate it while every other check in this file passed.
recomputed = critical_path(recorded_jobs, workflow_path)
recorded_path = ledger["criticalPath"]
if recorded_path.nil?
  errors << "the ledger records no criticalPath; feedback latency is the number FR-174 " \
            "argues from and it was left for each reader to re-derive"
elsif recomputed
  %w[full deferred].each do |tier|
    want = recomputed.dig(tier, "seconds")
    got = recorded_path.dig(tier, "seconds")
    if want != got
      errors << "criticalPath.#{tier} records #{got.inspect}s but the graph gives #{want}s; " \
                "re-run --refresh --write"
    end
    want_chain = recomputed.dig(tier, "chain")
    got_chain = recorded_path.dig(tier, "chain")
    if want_chain != got_chain
      errors << "criticalPath.#{tier} records the chain #{got_chain.inspect} but the graph " \
                "gives #{want_chain.inspect}"
    end
  end
  %w[tieredSteps tieredSeconds].each do |field|
    if recomputed[field] != recorded_path[field]
      errors << "criticalPath.#{field} records #{recorded_path[field].inspect} but ci.yml " \
                "gives #{recomputed[field].inspect}"
    end
  end
end

# 3. The budget. A ceiling with no written reason is the thing FR-140 forbids —
#    it would only ratify whatever the cost happened to be the day it was set.
budget = ledger["budget"] || {}
budgeted = budget["jobs"] || []
limit = budget["seconds"]
%w[reason reviewWhen decidedBy].each do |field|
  errors << "the budget has no #{field}; a ceiling nobody can review is not a decision" if budget[field].to_s.empty?
end

#    While any step is pending measurement the recorded total is known to be
#    missing seconds, and comparing an incomplete total against a ceiling would
#    report headroom that does not exist. Enforcement resumes by itself on the
#    refresh that measures the last pending step; it is not a switch anyone can
#    leave off, because `pendingMeasurement` entries are dropped automatically
#    once a run has measured them.
unenforceable = pending.keys.reject { |name| recorded_jobs.any? { |_j, r| (r["steps"] || {}).key?(name) } }

if limit.nil? || budgeted.empty?
  errors << "the budget must name the jobs it covers and a ceiling in seconds"
elsif unenforceable.any?
  # reported in the summary below, not an error: the ceiling is decided, the
  # measurement is not complete, and saying so is the honest state.
else
  missing_budgeted = budgeted.reject { |job| recorded_jobs.key?(job) }
  if missing_budgeted.any?
    errors << "budgeted job(s) with no cost record: #{missing_budgeted.join(', ')}"
  else
    total = budgeted.sum { |job| recorded_jobs[job]["seconds"].to_i }
    if total > limit
      errors << "governance costs #{total}s against a #{limit}s budget, over by #{total - limit}s"
      budgeted.each do |job|
        record = recorded_jobs[job]
        errors << "  #{job}: #{record['seconds']}s"
        (record["steps"] || {}).sort_by { |_name, value| -value.to_i }.first(8).each do |name, value|
          errors << "    #{value}s  #{name}"
        end
      end
      errors << "  a new gate has to fit, or the ceiling has to be raised in writing: #{budget['reviewWhen']}"
    end
  end
end

if errors.empty?
  total = budgeted.sum { |job| recorded_jobs[job]["seconds"].to_i }
  steps = recorded_jobs.values.sum { |record| (record["steps"] || {}).length }
  puts "CI step cost: PASS"
  puts "  #{steps} step(s) recorded across #{recorded_jobs.length} job(s) from run #{measurement['runId']}"
  if unenforceable.any?
    puts "  #{budgeted.join(' + ')} = #{total}s recorded against a #{limit}s budget, NOT ENFORCED:"
    unenforceable.sort.each { |name| puts "    '#{name}' has never run and has no measurement" }
    puts "  the ceiling binds again on the refresh that measures them"
  else
    puts "  #{budgeted.join(' + ')} = #{total}s against a #{limit}s budget " \
         "(#{((limit - total) * 100.0 / limit).round}% headroom)"
  end
  if recomputed
    full = recomputed.dig("full", "seconds")
    deferred = recomputed.dig("deferred", "seconds")
    puts "  critical path: #{full}s full / #{deferred}s deferred " \
         "(#{recomputed['tieredSteps']} tiered step(s), #{recomputed['tieredSeconds']}s)"
    puts "    longest chain: #{recomputed.dig('full', 'chain').join(' -> ')}"
  end
  exit 0
end

warn "CI step cost: FAIL"
errors.each { |error| warn "  #{error}" }
exit 1
