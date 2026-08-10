# Provider isolation, asserted at the point of use.
#
# A gate in `path-shadow` mode puts a fake provider on PATH ahead of the real
# one and then starts a daemon that will invoke whatever `claude` resolves to.
# FR-127 verified this by grepping the script for the `export PATH` line. FR-134
# reproduced what that accepts: comment the line out and the grep still matches
# it, because a commented line contains the same text. The gate certified an
# isolation that was no longer in effect.
#
# Grepping for a line cannot tell you whether the line ran. Resolving the
# provider can, so that is what this does — and it does it inside the gate,
# after the shadow is established, where the answer is the fact itself rather
# than a description of it.
#
# Sourced, not executed:
#   . "$REPO_ROOT/scripts/lib/provider_isolation.sh"
#   assert_provider_shadow "$QA_ROOT/bin" claude

# Fails the calling script unless every named provider resolves inside $bindir.
# A provider that resolves nowhere is fine — nothing can be invoked. A provider
# that resolves outside the shadow is the failure this exists to catch, and it
# is reported with both paths because "isolation failed" without the resolved
# path is a message you cannot act on.
assert_provider_shadow() {
  local bindir="$1"
  shift
  local providers=("$@")
  [[ ${#providers[@]} -eq 0 ]] && providers=(claude codex)

  if [[ ! -d "$bindir" ]]; then
    echo "FAIL: provider isolation: shadow directory does not exist: $bindir" >&2
    return 1
  fi

  local provider resolved rc=0
  for provider in ${providers[@]+"${providers[@]}"}; do
    resolved="$(command -v "$provider" 2>/dev/null || true)"
    if [[ -z "$resolved" ]]; then
      continue
    fi
    if [[ "$resolved" != "$bindir/"* ]]; then
      echo "FAIL: provider isolation: $provider resolves to $resolved, outside the shadow $bindir" >&2
      echo "      the PATH shadow is not in effect; this run could reach a real provider CLI" >&2
      rc=1
    fi
  done
  return $rc
}

# assert_provider_resolution <shell> <shell_arg> <bindir> [providers...]
#
# assert_provider_shadow resolves in THIS script's shell — a non-login bash.
# The runner spawns provider commands through its own shell and arg, and on
# macOS `-lc` changes the answer: the login shell runs path_helper, which
# reorders PATH so /etc/paths.d entries come first and the shadow directory is
# demoted below /opt/homebrew/bin — the parity gate's streaming step reached a
# real claude CLI through exactly that gap while the entry-level assertion
# above passed (FR-161). So resolve under the runner's declared shell and arg,
# and assert the answer lands inside the shadow. The shell/arg passed here must
# match what the governed manifest's RuntimePolicy declares; the two drifting
# apart re-opens the gap this closes.
assert_provider_resolution() {
  local shell="$1"
  local shell_arg="$2"
  local bindir="$3"
  shift 3
  local providers=("$@")
  [[ ${#providers[@]} -eq 0 ]] && providers=(claude codex)

  local provider resolved rc=0
  for provider in ${providers[@]+"${providers[@]}"}; do
    resolved="$("$shell" "$shell_arg" "command -v $provider" 2>/dev/null || true)"
    if [[ -z "$resolved" ]]; then
      continue
    fi
    if [[ "$resolved" != "$bindir/"* ]]; then
      echo "FAIL: provider isolation: under $shell $shell_arg, $provider resolves to $resolved, outside the shadow $bindir" >&2
      echo "      the runner's shell semantics defeat the PATH shadow (login-shell PATH reordering);" >&2
      echo "      a daemon-spawned provider command would reach a real CLI" >&2
      rc=1
    fi
  done
  return $rc
}
