# One answer to "am I running unattended?", for every governance tool that
# refuses to rewrite a reviewed artifact without a human present.
#
# Three tools each wrote `ENV.key?("CI")` separately (coordination-governance.rb,
# core-boundary.rb, doc-lifecycle.rb) and a fourth was about to. That predicate
# is narrower than it looks: it is a GitHub Actions and Travis convention, not a
# universal one. A self-hosted runner, a cron job, or a locally driven agent that
# does not export `CI` sails straight past it and rewrites the ledger nobody is
# watching — which turns the review gate into decoration, the exact outcome the
# guard exists to prevent.
#
# The risk today is small: no workflow invokes --write. The cost of closing it is
# smaller, and this is the only barrier there is.

module CiEnv
  # Variables that mean "no human is reading this output". GITHUB_ACTIONS and
  # GITLAB_CI are set by their runners whether or not CI is; BUILD_NUMBER covers
  # Jenkins, which sets neither.
  INDICATORS = %w[
    CI
    CONTINUOUS_INTEGRATION
    GITHUB_ACTIONS
    GITLAB_CI
    BUILDKITE
    CIRCLECI
    TEAMCITY_VERSION
    BUILD_NUMBER
  ].freeze

  module_function

  # True when any indicator is present and not explicitly falsey. `CI=false` is
  # how a developer says "treat this as interactive", and some runners export
  # empty values for variables they do not set.
  def unattended?(env = ENV)
    INDICATORS.any? do |name|
      value = env[name]
      !value.nil? && !value.empty? && !%w[0 false no].include?(value.downcase)
    end
  end

  # The indicators actually present, for a diagnostic that says why a write was
  # refused rather than leaving the caller to guess.
  def indicators(env = ENV)
    INDICATORS.select do |name|
      value = env[name]
      !value.nil? && !value.empty? && !%w[0 false no].include?(value.downcase)
    end
  end

  # The shared refusal. Callers pass the recovery instruction that fits them,
  # because "run --emit-baseline locally" and "run --emit-index locally" are
  # different sentences and neither generalises usefully.
  def refuse_unattended_write!(artifact, recovery)
    return unless unattended?

    warn "refusing --write under #{indicators.join(', ')}: " \
         "a regenerated #{artifact} must be reviewed by a human"
    warn recovery
    exit 2
  end
end
