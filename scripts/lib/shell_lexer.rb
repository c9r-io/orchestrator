# A shell lexer sufficient to tell code from the things that only look like it.
#
# It exists because `bash32-compat.rb` decided what a line contained by resetting
# its quote state at every newline. A `<< WORD` lookalike inside a region that
# began on an earlier line then read as a here-document opener, and the scanner
# discarded every remaining line of the file without saying so. FR-138 found two
# live instances: a `perl -e` replacement string containing `<<EOF`, and — the
# one worth remembering — `hosting << job_name` inside the `ruby -e` block that
# asserts CI runs the gate on a bash 3.2 host. The line proving the gate has a
# macOS host is the line that hid the gate's own last 16 lines from it.
#
# This is the same failure `rust_lexer.rb` was written for one FR earlier, and
# the same fix: carry lexical state across lines. Shell needs one thing Rust does
# not, and getting it wrong is what makes the second instance survive a naive
# port — see `open_substitution` below.
#
# Scope: lexical structure only. Quoting, comments, here-document extent. It does
# not parse commands and nothing here needs it to.
#
# Ruby 2.6 compatible on purpose: macOS system ruby is 2.6, and the macOS CI leg
# is the only host where the bash 3.2 semantics this feeds are real.

module ShellLexer
  module_function

  # Quoting state, carried across lines.
  #
  # A stack rather than two booleans, because `$( )` opens a fresh quoting
  # context: quotes inside a command substitution are independent of the quotes
  # around it. `X="$(ruby -e '...')"` is a double-quoted region whose interior
  # begins a *single*-quoted one, and a flat tracker reads that `'` as an
  # ordinary character inside double quotes. Every subsequent quote is then
  # mispaired, which is exactly how the `ruby -e` instance escaped.
  class State
    attr_reader :heredoc, :heredoc_line, :heredoc_lines

    def initialize
      @stack = [{ quote: nil, depth: 0 }]
      @heredoc = nil
      @heredoc_line = nil
      # Counted as they are dropped, not derived from `total - scanned`. A
      # derived count makes "scanned + heredoc == total" an identity that no
      # defect can violate, which is a check that reads like evidence and is not.
      @heredoc_lines = 0
    end

    def drop_heredoc_line
      @heredoc_lines += 1
    end

    def context
      @stack.last
    end

    def quote
      context[:quote]
    end

    def quote=(value)
      context[:quote] = value
    end

    def nested?
      @stack.length > 1
    end

    def open_substitution
      @stack.push(quote: nil, depth: 0)
    end

    def close_substitution
      @stack.pop if nested?
    end

    def open_heredoc(word, line)
      return if @heredoc

      @heredoc = word
      @heredoc_line = line
    end

    def close_heredoc
      @heredoc = nil
      @heredoc_line = nil
    end

    def in_heredoc?
      !@heredoc.nil?
    end
  end

  # Returns [[line_number, code], ...] and the terminal State.
  #
  # Here-document bodies are dropped: they are data to the enclosing script, and
  # this repository's QA wrappers write their hazardous fixtures that way. The
  # State comes back so the caller can ask whether the file ended inside one —
  # a file that does was never fully scanned, and saying so is the whole point.
  def code_lines(text)
    state = State.new
    result = []

    text.lines.each_with_index do |raw, index|
      line = raw.chomp
      number = index + 1

      if state.in_heredoc?
        state.drop_heredoc_line
        state.close_heredoc if line.strip == state.heredoc
        next
      end

      result << [number, scan_line(line, state, number)]
    end

    [result, state]
  end

  # One line, advancing `state`. Returns the line with comments and single-quoted
  # regions blanked, preserving length so callers can still reason about columns.
  #
  # Single-quoted regions are blanked and double-quoted ones are not. This is
  # where shell parts company with Rust, and reversing it breaks the gate: the
  # canonical hazard is `printf '%s\n' "${args[@]}"`, whose dangerous half lives
  # *inside* double quotes because shell still expands there. In Rust every
  # string literal is inert, so `rust_lexer.rb` can blank them all; here only the
  # single-quoted ones are inert.
  def scan_line(line, state, number)
    out = +""
    index = 0
    # Where the current single-quoted run starts in `out`, or nil.
    quoted_from = state.quote == :single ? 0 : nil

    while index < line.length
      char = line[index]
      quote = state.quote

      if char == "\\" && quote != :single
        out << char << (line[index + 1] || " ")
        index += 2
        next
      end

      if quote == :single
        if char == "'"
          state.quote = nil
          out[quoted_from..-1] = " " * (out.length - quoted_from) if quoted_from
          quoted_from = nil
        end
        out << char
        index += 1
        next
      end

      # An apostrophe inside a double-quoted string is an ordinary character.
      # Treating it as a quote opener mispairs everything after it, which blanks
      # real code and reports a clean file.
      if char == "'" && quote.nil?
        state.quote = :single
        out << char
        quoted_from = out.length
        index += 1
        next
      end

      if char == '"'
        state.quote = (quote == :double ? nil : :double)
        out << char
        index += 1
        next
      end

      # `$(` resets quoting; `$((` is arithmetic and balances in place.
      if char == "$" && line[index + 1] == "(" && line[index + 2] != "("
        state.open_substitution
        out << char << "("
        index += 2
        next
      end

      if char == "(" && quote != :double
        state.context[:depth] += 1
      elsif char == ")" && quote != :double
        if state.context[:depth].positive?
          state.context[:depth] -= 1
        else
          state.close_substitution
        end
      elsif char == "#" && quote.nil? && (index.zero? || line[index - 1] =~ /\s/)
        # A comment runs to end of line. `'#'` and `"...#..."` do not.
        break
      elsif char == "<" && quote.nil? && line[index + 1] == "<"
        word = line[index..-1][/\A<<-?\s*(?!<)(["']?)([A-Za-z_][A-Za-z0-9_]*)\1/, 2]
        state.open_heredoc(word, number) if word
      end

      out << char
      index += 1
    end

    out[quoted_from..-1] = " " * (out.length - quoted_from) if state.quote == :single && quoted_from
    out
  end
end
