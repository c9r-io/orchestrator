# Shared Rust source scanning and ledger serialisation for the governance gates.
#
# This file is a library, not a gate. It lives under scripts/lib rather than
# scripts/qa/lib because the FR-127 enforcement surface enumerates
# `ls scripts/qa/*.sh scripts/qa/*.rb` non-recursively: a ruby file under
# scripts/qa/ that the manifest cannot see would be a gate-shaped file outside
# governance. Keeping the library outside the governed directory says what it is.
#
# It exists because two ledgers now count the same source tree —
# coordination-collapse-ledger.json (FR-124/125) and core-boundary-ledger.json
# (FR-130) — and the scan is not incidental to the number. Stripping inline
# cfg(test) modules moves the core rusqlite count from 237 to 200 and the file
# count from 43 to 37, so two implementations that drift produce two different
# reviewed states while both look correct.

require "json"
require "pathname"

module RustSource
  module_function

  def relative_path(root, path)
    Pathname.new(path).relative_path_from(root).to_s
  end

  # Non-test Rust source under core/src and crates/*/src, plus the member
  # Cargo.toml manifests. Files under a `tests` directory and files whose
  # basename matches `test*.rs` are excluded wholesale; inline test modules are
  # handled by strip_test_modules, which callers apply per file.
  def rust_source_files(repo_root)
    roots = [repo_root.join("core/src")]
    roots.concat(Dir[repo_root.join("crates/*/src").to_s].map { |path| Pathname.new(path) })
    files = roots.flat_map do |root|
      Dir[root.join("**/*").to_s].each_with_object([]) do |path, collected|
        pathname = Pathname.new(path)
        next unless pathname.file?
        next unless pathname.extname == ".rs"
        relative = pathname.relative_path_from(repo_root).to_s
        next if relative.split("/").include?("tests")
        next if pathname.basename.to_s.match?(/test.*\.rs\z/)
        collected << pathname
      end
    end
    manifests = [repo_root.join("core/Cargo.toml")]
    manifests.concat(Dir[repo_root.join("crates/*/Cargo.toml").to_s].map { |path| Pathname.new(path) })
    files + manifests.select(&:file?)
  end

  # Both ledgers' scope prose excludes inline cfg(test) modules. Matching a
  # single trailing `mod tests { ... }` per file does not implement that: a test
  # module named anything else, or followed by production code, was scanned in
  # full. FR-128 found ten such lines (nine PipelineVariables in the scheduler
  # item_executor tests, one output_json_path in task_repository) inflating the
  # ratchets with test-only usage. Brace-matching every cfg(test) module makes
  # the implementation mean what the scope says.
  def strip_test_modules(source)
    lines = source.lines
    excluded = []
    index = 0
    while index < lines.length
      attribute = lines[index].match?(/^\s*#\[cfg\(test\)\]/)
      declaration = lines[index + 1]
      if attribute && declaration && declaration.match?(/^\s*(pub(\([^)]*\))?\s+)?mod\s+\w+\s*\{/)
        depth = 0
        opened = false
        cursor = index + 1
        while cursor < lines.length
          depth += lines[cursor].count("{") - lines[cursor].count("}")
          opened ||= lines[cursor].include?("{")
          break if opened && depth <= 0
          cursor += 1
        end
        excluded << (index..cursor)
        index = cursor
      end
      index += 1
    end
    return source if excluded.empty?

    lines.each_with_index.reject do |_, position|
      excluded.any? { |range| range.cover?(position) }
    end.map(&:first).join
  end

  # Reads a source file the way the ledgers count it.
  def scannable_source(path)
    source = File.read(path)
    path.extname == ".rs" ? strip_test_modules(source) : source
  end

  # Ruby's JSON.pretty_generate writes an empty array as "[\n\n]". The reviewed
  # ledgers use "[]", so a --write round trip would otherwise move lines that no
  # reviewer asked to change and bury the real edit.
  def ledger_json(value)
    JSON.pretty_generate(value)
      .gsub(/\[\n\s*\n\s*\]/, "[]")
      .gsub(/\{\n\s*\n\s*\}/, "{}") + "\n"
  end
end
