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
require_relative "rust_lexer"

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
    manifests = [repo_root.join("core/Cargo.toml")]
    manifests.concat(Dir[repo_root.join("crates/*/Cargo.toml").to_s].map { |path| Pathname.new(path) })
    rust_files_under(repo_root, roots) + manifests.select(&:file?)
  end

  # The exclusion rules on their own, so a caller that discovers its roots
  # differently does not have to restate them.
  #
  # rust_source_files above hardcodes core/src plus crates/*/src. That is the
  # right scope for the two ledgers that count core, and the wrong one for
  # anything asking a question about the workspace: a member declared outside
  # crates/ is simply not scanned, silently. FR-136's gate derives its roots from
  # the [workspace] members list and calls this directly, so the discovery is its
  # own and only the counting is shared.
  #
  # A root may be a directory or a single file. FR-139 needed the second form:
  # a Cargo build script is one file at the member root, `Dir[file/**/*]` yields
  # nothing for it, and a caller working around that would have had to restate
  # the exclusion rules — which is how two implementations of one scope begin.
  # Both forms run through the same filter below.
  def rust_files_under(repo_root, roots)
    roots.flat_map do |root|
      pathname = Pathname.new(root)
      candidates = pathname.file? ? [pathname] : Dir[pathname.join("**/*").to_s]
      candidates.each_with_object([]) do |path, collected|
        candidate = Pathname.new(path)
        next unless candidate.file?
        next unless candidate.extname == ".rs"
        relative = candidate.relative_path_from(repo_root).to_s
        next if relative.split("/").include?("tests")
        next if candidate.basename.to_s.match?(/test.*\.rs\z/)
        collected << candidate
      end
    end
  end

  # Both ledgers' scope prose excludes inline cfg(test) modules. Matching a
  # single trailing `mod tests { ... }` per file does not implement that: a test
  # module named anything else, or followed by production code, was scanned in
  # full. FR-128 found ten such lines (nine PipelineVariables in the scheduler
  # item_executor tests, one output_json_path in task_repository) inflating the
  # ratchets with test-only usage. Brace-matching every cfg(test) module makes
  # the implementation mean what the scope says.
  # `masked` lets a caller that has already paid for RustLexer.mask_literals hand
  # the result in rather than paying for it twice. FR-141's gate needs both the
  # stripped source and a stripped *masked* copy of it, and masking is the whole
  # cost of these scans: 13 seconds across the workspace, which that gate was
  # spending four times over. Passing the masked source as both arguments strips
  # the masked copy itself. Existing callers are unaffected.
  def strip_test_modules(source, masked = nil)
    lines = source.lines
    # Braces are counted on a lexically masked copy — see RustLexer for why a
    # per-line regex is not enough — while the ranges index the real lines.
    counted = (masked || RustLexer.mask_literals(source)).lines
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
          depth += counted[cursor].count("{") - counted[cursor].count("}")
          opened ||= counted[cursor].include?("{")
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

  # A `cfg(test)` module whose depth never returns to zero has no end, so
  # strip_test_modules excludes everything after it. That is invisible in the
  # ledger numbers — the hidden lines simply stop being counted — so it is
  # asserted directly. Returns [[path, line_number], ...].
  def unclosed_test_modules(repo_root)
    rust_source_files(repo_root).each_with_object([]) do |path, found|
      next unless path.extname == ".rs"

      lines = File.read(path).lines
      counted = RustLexer.mask_literals(lines.join).lines
      index = 0
      while index < lines.length
        declaration = lines[index + 1]
        if lines[index].match?(/^\s*#\[cfg\(test\)\]/) && declaration &&
           declaration.match?(/^\s*(pub(\([^)]*\))?\s+)?mod\s+\w+\s*\{/)
          depth = 0
          opened = false
          cursor = index + 1
          closed = false
          while cursor < lines.length
            depth += counted[cursor].count("{") - counted[cursor].count("}")
            opened ||= counted[cursor].include?("{")
            if opened && depth <= 0
              closed = true
              break
            end
            cursor += 1
          end
          found << [relative_path(repo_root, path), index + 1] unless closed
          index = cursor
        end
        index += 1
      end
    end
  end

  # Reads a source file the way the ledgers count it.
  def scannable_source(path)
    source = File.read(path)
    path.extname == ".rs" ? strip_test_modules(source) : source
  end

  # The same thing with everything that is not code blanked out: comments, doc
  # comments, char literals and strings of every raw-hash depth become spaces,
  # so line structure and offsets are preserved and only the code remains.
  #
  # This exists because a ratchet counting an identifier counted the identifier
  # in prose too. DD-142 recorded it as a known limit of the `rusqlite` ruler
  # ("a comment mentioning rusqlite counts") and DD-148 recorded the instance
  # that makes it concrete: a doc comment written to explain that the driver
  # conversion had been *removed* named the impl and put the file back on the
  # ledger, and the workaround was to stop spelling the type's path — precision
  # traded for a metric. Measured at FR-158, prose was 52% of one coordinate:
  # capturesOrJsonPath read 23 lines and reads 11 masked, because the rejection
  # diagnostics that *delete* the surface name it in their message strings.
  #
  # Masking is paid for once. `strip_test_modules(masked, masked)` strips the
  # masked copy using itself for brace depth, which is the shape FR-141 added
  # the second parameter for; masking is the entire cost of these scans (13s
  # across the workspace, the largest item in the governance CI bill, FR-140).
  #
  # Not for every ruler. A pattern whose subject *is* a string literal must read
  # the unmasked source or it measures nothing: persistence-dependency.rb's
  # SQL_STATEMENT anchors on the opening quote, and counting it here would take
  # 518 statements to 0 without failing anything. Mask what is an identifier;
  # leave what is a literal. Non-Rust files are returned unchanged, so a `#`
  # comment in a Cargo.toml is still counted — TOML has no lexer here.
  def masked_scannable_source(path)
    source = File.read(path)
    return source unless path.extname == ".rs"

    masked = RustLexer.mask_literals(source)
    strip_test_modules(masked, masked)
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
