# A Rust lexer sufficient to mask out everything that must not be counted as code.
#
# It exists because both governance ledgers decide what a `#[cfg(test)]` module
# covers by counting braces, and a brace inside a string literal or a comment is
# not a brace. FR-134 recorded the failure this produces: `.body("{")` leaves the
# depth counter permanently above zero, the module's range runs to end of file,
# and every production line after it disappears from the scan. The ratchet then
# holds still while the thing it counts moves.
#
# The obvious fix — strip literals line by line with a regular expression — is
# worse than the defect, and the repository has the case that proves it.
# `crates/orchestrator-scheduler/src/scheduler/item_generate.rs:199` opens
# `r#"{"items": [` and closes it three lines later. A per-line matcher sees an
# unbalanced `}` on the closing line, decides the module ended 245 lines early,
# and hands 7 lines of *test* fixture to the ratchet as production usage. Masking
# has to carry state across lines or it trades under-counting for over-counting.
#
# Scope: this masks lexical structure only. It does not parse items, and nothing
# here needs it to.

module RustLexer
  module_function

  # Returns `source` with every byte that is not code replaced by a space,
  # preserving length and line structure so callers can still index by line.
  # Masked: line comments, block comments (nested, as Rust allows), char
  # literals, byte/normal strings with escapes, and raw strings of any hash
  # depth. Everything else is passed through unchanged.
  #
  # Walked as an array of characters rather than by indexing the String. The
  # algorithm below is unchanged; only the storage is. `String#[]` on a
  # multibyte-capable string is not the O(1) operation it looks like, and this
  # loop performs one per character over every tracked Rust file: measured on
  # the largest of them (116 804 characters), a bare walk doing no work at all
  # cost 892 ms through `String#[]` and 15 ms through `chars`. That difference,
  # multiplied by the four governance gates that call this and by the fixture
  # cases that re-run each of them, was the largest single item in the
  # governance CI bill (FR-140, DD-153).
  def mask_literals(source)
    source = source.chars
    out = source.dup
    length = source.length
    index = 0

    blank = lambda do |from, to|
      (from...to).each { |position| out[position] = " " unless source[position] == "\n" }
    end

    while index < length
      char = source[index]

      # Line comment: to the newline, which stays.
      if char == "/" && source[index + 1] == "/"
        stop = index_of(source, "\n", index) || length
        blank.call(index, stop)
        index = stop
        next
      end

      # Block comment: Rust nests them, so track depth rather than scanning for
      # the first `*/`.
      if char == "/" && source[index + 1] == "*"
        depth = 1
        cursor = index + 2
        while cursor < length && depth.positive?
          if source[cursor] == "/" && source[cursor + 1] == "*"
            depth += 1
            cursor += 2
          elsif source[cursor] == "*" && source[cursor + 1] == "/"
            depth -= 1
            cursor += 2
          else
            cursor += 1
          end
        end
        blank.call(index, cursor)
        index = cursor
        next
      end

      # Raw string: r"..." / r#"..."# / br##"..."## — no escapes, terminated by
      # a quote followed by exactly the opening number of hashes.
      raw = raw_string_start(source, index)
      if raw
        hashes, body_start = raw
        stop = index_of_terminator(source, hashes, body_start)
        stop = stop ? stop + hashes + 1 : length
        blank.call(index, stop)
        index = stop
        next
      end

      # Normal or byte string.
      if char == "\"" || (char == "b" && source[index + 1] == "\"")
        cursor = char == "\"" ? index + 1 : index + 2
        while cursor < length
          if source[cursor] == "\\"
            cursor += 2
          elsif source[cursor] == "\""
            cursor += 1
            break
          else
            cursor += 1
          end
        end
        blank.call(index, cursor)
        index = cursor
        next
      end

      # Char literal, which has to be told apart from a lifetime. `'a` is a
      # lifetime; `'a'` is a char. Only the closed form is a literal, and only
      # the closed form can contain a brace.
      if char == "'" && (stop = char_literal_end(source, index))
        blank.call(index, stop)
        index = stop
        next
      end

      index += 1
    end

    out.join
  end

  # `Array#index` takes no start offset and `String#index` is not available once
  # the source is an array of characters, so both searches the masker needs are
  # spelled out here. Both are plain forward scans; neither is on a hot path
  # that the character walk was not already paying for.
  def index_of(source, char, from)
    cursor = from
    length = source.length
    while cursor < length
      return cursor if source[cursor] == char

      cursor += 1
    end
    nil
  end

  # The end of a raw string body: a quote followed by exactly `hashes` hashes.
  # Returns the offset of that quote, or nil when the file ends first.
  def index_of_terminator(source, hashes, from)
    cursor = from
    length = source.length
    while (cursor = index_of(source, "\"", cursor))
      matched = 0
      matched += 1 while matched < hashes && source[cursor + 1 + matched] == "#"
      return cursor if matched == hashes

      cursor += 1
    end
    nil
  end

  # `r`/`br` followed by zero or more `#` then a quote. Returns [hashes, offset
  # just past the opening quote], or nil.
  def raw_string_start(source, index)
    cursor = index
    cursor += 1 if source[cursor] == "b"
    return nil unless source[cursor] == "r"

    cursor += 1
    hashes = 0
    while source[cursor] == "#"
      hashes += 1
      cursor += 1
    end
    return nil unless source[cursor] == "\""

    # `r` must start a token: `str` and `for` end in letters that would
    # otherwise look like a raw string prefix.
    previous = index.positive? ? source[index - 1] : nil
    return nil if previous && previous.match?(/[A-Za-z0-9_]/)

    [hashes, cursor + 1]
  end

  # Offset just past the closing quote of a char literal at `index`, or nil when
  # this apostrophe opens a lifetime or a label instead.
  def char_literal_end(source, index)
    cursor = index + 1
    return nil if cursor >= source.length

    if source[cursor] == "\\"
      cursor += 1
      # \x41, \u{1F600}, \n, \\, \' — consume to the closing quote, bounded so a
      # stray backslash cannot run away with the rest of the file.
      limit = [cursor + 12, source.length - 1].min
      while cursor <= limit
        return cursor + 1 if source[cursor] == "'"

        cursor += 1
      end
      return nil
    end

    # A single character followed by a quote. Anything longer is a lifetime.
    return cursor + 2 if source[cursor + 1] == "'"

    nil
  end
end
