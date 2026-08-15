#!/usr/bin/env ruby
# frozen_string_literal: true

# FR-155: generate the Feature Request registry from the complete HEAD ancestry.

require "json"
require "open3"
require "pathname"

BEGIN_MARK = "<!-- BEGIN GENERATED FR REGISTRY -->"
END_MARK = "<!-- END GENERATED FR REGISTRY -->"
POLICY = "config/governance/fr-registry-legacy.json"
README = "docs/feature_request/README.md"

def fail!(message)
  warn(message)
  exit(1)
end

def git(root, *args, allow_failure: false)
  stdout, stderr, status = Open3.capture3("git", "-C", root.to_s, *args)
  return [stdout, status.success?] if allow_failure
  fail!("git #{args.join(' ')} failed: #{stderr.strip}") unless status.success?
  stdout
end

def fr_number(path)
  File.basename(path).match(/\Afr[-_ ]?0*(\d+).*\.md\z/i)&.captures&.first&.to_i
end

# Both walks pass --full-history, and they have to pass the *same* thing.
#
# `git log -- <path>` simplifies history by default: at a merge whose tree is
# TREESAME to one parent it follows that parent only, so a file added and then
# deleted on a side branch can disappear from the walk entirely. A pull request
# is exactly that shape — actions/checkout builds refs/pull/N/merge and every CI
# run of this tool has HEAD on a merge commit — while a developer running it on
# the branch tip does not, which is why this passed locally and failed in CI on
# the first pull request this repository ever opened.
#
# The two walks must agree by construction, not by luck: `historical_paths`
# decides which paths exist to explain and `content_at_latest_existing_revision`
# has to find a revision for each one. Under simplification the first found
# FR-169 (its --diff-filter forces the diff) and the second did not, and the
# disagreement surfaced as "history listed X, but no revision contains it" —
# a message about the tree that was really about the walk.
def historical_paths(root)
  output = git(root, "log", "--full-history", "--format=", "--name-only", "--diff-filter=ACDMRT", "HEAD", "--", "docs/feature_request")
  output.lines.map(&:strip).reject(&:empty?).select { |path| fr_number(path) }.uniq.sort
end

def content_at_latest_existing_revision(root, path)
  commits = git(root, "log", "--full-history", "--format=%H", "HEAD", "--", path).lines.map(&:strip)
  commits.each do |commit|
    content, found = git(root, "show", "#{commit}:#{path}", allow_failure: true)
    return content if found
  end
  fail!("history listed #{path}, but no revision contains it")
end

def present_at_head?(root, path)
  _output, found = git(root, "cat-file", "-e", "HEAD:#{path}", allow_failure: true)
  found
end

def title_from(content, path)
  heading = content.lines.find { |line| line.match?(/^#\s+/) }
  return File.basename(path, ".md").tr("-_", " ") unless heading

  title = heading.sub(/^#\s+/, "").strip
  title = title.sub(/\AFR[-_ ]?0*\d+\s*(?::|—|-)?\s*/i, "")
  title = title.sub(/\AFeature Request\s*(?::|—|-)?\s*/i, "")
  title.empty? ? File.basename(path, ".md").tr("-_", " ") : title
end

def priority_from(content)
  match = content.match(/(?:\*\*)?(?:优先级|Priority)(?:\*\*)?\s*[:：]\s*`?(P[0-3])`?/i)
  match ? match[1].upcase : "—"
end

def status_from(content)
  match = content.match(/(?:\*\*)?(?:状态|Status)(?:\*\*)?\s*[:：]\s*`?([^`\n]+)`?/i)
  value = match&.captures&.first&.strip
  return "Proposed" if value.to_s.empty?

  normalized = value.downcase.tr("_", " ").gsub(/\s+/, " ")
  return "In Progress" if normalized.include?("progress")
  return "Implemented" if normalized.include?("implemented")
  return "Closed" if normalized.include?("closed")
  return "Proposed" if normalized.include?("proposed")

  value
end

def legacy_entries(root, historical_numbers)
  policy_path = root.join(POLICY)
  policy = JSON.parse(File.read(policy_path))
  entries = policy.fetch("entries")
  fail!("#{policy_path}: entries must be an array") unless entries.is_a?(Array)

  seen = {}
  entries.each do |entry|
    required = %w[id title priority status reason]
    missing = required.reject { |key| entry[key].is_a?(String) && !entry[key].strip.empty? }
    fail!("#{policy_path}: invalid legacy entry #{entry} (missing #{missing.join(', ')})") unless missing.empty?
    match = entry.fetch("id").match(/\AFR-(\d{3})\z/)
    fail!("#{policy_path}: legacy id must be canonical FR-NNN: #{entry['id']}") unless match
    number = match[1].to_i
    fail!("#{policy_path}: duplicate legacy id #{entry['id']}") if seen[number]
    fail!("#{policy_path}: #{entry['id']} has file history and is no longer a legacy exception") if historical_numbers.include?(number)
    fail!("#{policy_path}: #{entry['id']} reason is too short") if entry.fetch("reason").length < 20
    seen[number] = true
  end
  entries
end

def registry(root)
  shallow = git(root, "rev-parse", "--is-shallow-repository").strip
  fail!("FR registry requires complete history; repository is shallow") unless shallow == "false"

  paths = historical_paths(root)
  fail!("FR registry history scan found no FR documents") if paths.empty?
  grouped = paths.group_by { |path| fr_number(path) }
  legacy = legacy_entries(root, grouped.keys)

  rows = grouped.map do |number, number_paths|
    sorted_paths = number_paths.sort
    current_path = sorted_paths.find { |path| present_at_head?(root, path) }
    evidence_path = current_path || sorted_paths.first
    content = if current_path
                git(root, "show", "HEAD:#{current_path}")
              else
                content_at_latest_existing_revision(root, evidence_path)
              end
    collision = if sorted_paths.length > 1
                  "collision (#{sorted_paths.length}): #{sorted_paths.map { |path| File.basename(path) }.join('; ')}"
                else
                  "git history"
                end
    {
      "number" => number,
      "id" => format("FR-%03d", number),
      "title" => title_from(content, evidence_path),
      "priority" => priority_from(content),
      "status" => current_path ? status_from(content) : "Closed",
      "source" => collision
    }
  end

  legacy.each do |entry|
    rows << {
      "number" => entry.fetch("id").delete_prefix("FR-").to_i,
      "id" => entry.fetch("id"),
      "title" => entry.fetch("title"),
      "priority" => entry.fetch("priority"),
      "status" => entry.fetch("status"),
      "source" => "legacy exception: #{entry.fetch('reason')}"
    }
  end

  collisions = grouped.count { |_number, number_paths| number_paths.length > 1 }
  [rows.sort_by { |row| row.fetch("number") }, paths.length, grouped.length, legacy.length, collisions]
end

def escape_cell(value)
  value.to_s.gsub("|", "\\|").gsub(/\s+/, " ").strip
end

def render(root)
  rows, path_count, history_count, legacy_count, collision_count = registry(root)
  table = rows.map do |row|
    "| #{row.fetch('id')} | #{escape_cell(row.fetch('title'))} | #{row.fetch('priority')} | #{row.fetch('status')} | #{escape_cell(row.fetch('source'))} |"
  end
  <<~MARKDOWN.chomp
    #{BEGIN_MARK}
    > 由 `scripts/lib/fr_registry.rb` 从完整 `HEAD` 祖先历史生成：#{history_count} 个历史编号 / #{path_count} 条历史路径，另有 #{legacy_count} 条无 FR 文件历史的审阅例外；#{collision_count} 个编号存在多路径碰撞。浅克隆拒绝生成。

    | ID | 标题 | 优先级 | 状态 | 来源 / 碰撞 |
    |----|------|--------|------|-------------|
    #{table.join("\n")}
    #{END_MARK}
  MARKDOWN
end

def replace_registry(readme, block)
  if readme.include?(BEGIN_MARK) || readme.include?(END_MARK)
    pattern = /#{Regexp.escape(BEGIN_MARK)}.*?#{Regexp.escape(END_MARK)}/m
    fail!("#{README}: generated registry markers are incomplete") unless readme.match?(pattern)
    return readme.sub(pattern, block)
  end

  pattern = /(## 当前条目\s*\n).*?(?=\n## 说明\s*$)/m
  fail!("#{README}: cannot locate 当前条目 section") unless readme.match?(pattern)
  readme.sub(pattern) { "#{$1}\n#{block}\n" }
end

mode = ARGV.shift
root = Pathname(ARGV.shift || Dir.pwd).expand_path
block = render(root)
readme_path = root.join(README)

case mode
when "render"
  puts(block)
when "check"
  actual = File.read(readme_path)
  expected = replace_registry(actual, block)
  fail!("#{README} differs from HEAD history; run scripts/lib/fr_registry.rb write") unless actual == expected
when "write"
  if %w[CI CONTINUOUS_INTEGRATION GITHUB_ACTIONS GITLAB_CI BUILDKITE CIRCLECI].any? { |name| !ENV[name].to_s.empty? }
    fail!("refusing to rewrite the FR registry under CI")
  end
  actual = File.read(readme_path)
  expected = replace_registry(actual, block)
  File.write(readme_path, expected)
else
  fail!("usage: fr_registry.rb {render|check|write} [repo-root]")
end
