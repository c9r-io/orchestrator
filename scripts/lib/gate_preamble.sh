# The commands a QA gate checks for before it does anything.
#
# That preamble is what decides whether a gate runs at all: it exits there,
# having asserted nothing, and the job goes red for a reason that looks nothing
# like the contract the gate was written to verify. FR-134 found three gates in
# that state, in two jobs, for a full FR cycle.
#
# Shared because two gates need it and would otherwise answer the same question
# two ways. test-ci-environment-parity.sh used "does the file mention cargo" for
# a while, which excluded test-qa-gate-surface.sh — a file that mentions cargo
# inside a regular expression and never runs it. Mentioning and running are
# different, which is this FR's whole subject.
#
# Sourced, not executed:
#   . "$REPO_ROOT/scripts/lib/gate_preamble.sh"
#   gate_required_commands scripts/qa/test-foo.sh

# Prints one command per line, sorted and deduplicated.
#
# Two shapes exist in this repository and both are handled: a `for X in a b c`
# loop whose body tests `command -v "$X"`, and a bare `command -v name`.
# Comments are stripped first, so prose describing a preamble is not read as
# one. A ruby gate is invoked as `ruby <path>` and needs ruby whether its text
# says so or not.
gate_required_commands() {
  local file="$1"
  {
    [[ "$file" == *.rb ]] && echo ruby
    [[ "$file" == *.sh ]] && sed -E 's/(^|[[:space:]])#.*$//' "$file" | awk '
      match($0, /^[[:space:]]*for[[:space:]]+[A-Za-z_][A-Za-z0-9_]*[[:space:]]+in[[:space:]]+/) {
        header = $0
        sub(/^[[:space:]]*for[[:space:]]+/, "", header)
        split(header, parts, /[[:space:]]+/)
        variable = parts[1]
        body = $0
        sub(/^.*[[:space:]]in[[:space:]]+/, "", body)
        sub(/;.*$/, "", body)
        pending_variable = variable
        pending_words = body
        next
      }
      pending_variable != "" && $0 ~ ("command -v \"\\$" pending_variable "\"") {
        count = split(pending_words, words, /[[:space:]]+/)
        for (index_ = 1; index_ <= count; index_++) {
          if (words[index_] != "" && words[index_] !~ /^[$"]/) print words[index_]
        }
        pending_variable = ""
        next
      }
      match($0, /command -v [A-Za-z][A-Za-z0-9_.-]*/) {
        print substr($0, RSTART + 11, RLENGTH - 11)
      }
    '
  } | LC_ALL=C sort -u
}

# True when the gate declares a dependency on the given command.
gate_requires() {
  grep -qxF "$2" <<< "$(gate_required_commands "$1")"
}
