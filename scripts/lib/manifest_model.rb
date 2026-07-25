#!/usr/bin/env ruby
#
# Per-agent view of a manifest bundle, for the provider isolation check.
#
# The `fixture-pinned` contract in config/governance/qa-gate-surface.json reads
# "Every claude/codex agent in the named fixture bundle also declares
# `binary: fake-*`". FR-127 implemented it by counting whole-file lines: as many
# `binary: fake-` matches as `provider:` matches meant pinned. That is a
# different claim. FR-134 reproduced the gap by appending an unpinned
# `provider: claude` agent alongside an unrelated agent carrying
# `binary: fake-decoy` — two providers, two pins, gate green, and one real CLI
# reachable from CI.
#
# A count over a file cannot express a per-object property. This walks the
# document stream and reports each agent's own provider and own binary, so the
# check can assert the association the contract actually claims.
#
# Usage:
#   manifest_model.rb agents <bundle>     name<TAB>provider<TAB>binary, one per agent
#   manifest_model.rb unpinned <bundle>   names of claude/codex agents with no fake pin

require "yaml"
require "date"

module ManifestModel
  PINNED = /\Afake-/.freeze
  REAL_PROVIDERS = %w[claude codex].freeze

  module_function

  def documents(path)
    YAML.load_stream(File.read(path)).compact
  rescue Psych::SyntaxError => error
    warn "#{path}: #{error.message}"
    exit 1
  end

  # Every Agent in the bundle, with the driver fields that decide isolation.
  # A bundle may also carry Workflows, Projects and Secrets; they have no driver
  # and are not the subject of the contract.
  def agents(path)
    documents(path).select { |document| document.is_a?(Hash) && document["kind"] == "Agent" }
      .map do |document|
        spec = document["spec"] || {}
        driver = spec["driver"] || {}
        {
          "name" => (document["metadata"] || {})["name"].to_s,
          # Pre-driver bundles put the provider directly on the spec. Reading
          # only spec.driver.provider would silently report those as having no
          # provider at all, which is the permissive direction.
          "provider" => (driver["provider"] || spec["provider"]).to_s,
          "binary" => (driver["binary"] || spec["binary"]).to_s
        }
      end
  end

  # Agents that name a real provider CLI without pinning it to a fake binary.
  # These are the ones that can reach credentials and quota.
  def unpinned(path)
    agents(path).select do |agent|
      REAL_PROVIDERS.include?(agent["provider"]) && !agent["binary"].match?(PINNED)
    end
  end
end

if $PROGRAM_NAME == __FILE__
  command = ARGV.shift
  bundle = ARGV[0]
  unless bundle && File.file?(bundle)
    warn "usage: manifest_model.rb {agents|unpinned} <bundle>"
    exit 2
  end

  case command
  when "agents"
    ManifestModel.agents(bundle).each do |agent|
      puts "#{agent['name']}\t#{agent['provider']}\t#{agent['binary']}"
    end
  when "unpinned"
    unpinned = ManifestModel.unpinned(bundle)
    unpinned.each { |agent| puts "#{agent['name']}\t#{agent['provider']}" }
    exit(unpinned.empty? ? 1 : 0)
  else
    warn "usage: manifest_model.rb {agents|unpinned} <bundle>"
    exit 2
  end
end
