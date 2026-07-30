#!/usr/bin/env bash
#
# The workflow-ID cross-reference check from scripts/qa-doc-lint.sh.
# Sourced by a gate, never executed.
#
# Every `--workflow <id>` in an orchestrator QA document must name a Workflow
# some fixture bundle defines. The point is that the commands in a QA document
# can be pasted into a shell and run; an id no bundle defines is a command that
# cannot work.
#
# Scoped to docs/qa/orchestrator/*.md: scripts and self-bootstrap docs embed
# inline workflow definitions that are not in fixture bundles.
#
# FR-149 narrowed the scope again, to `lifecycle: active` documents, because the
# premise above does not hold for a superseded one. A superseded document
# describes a mechanism that was removed, and the fixtures it names were deleted
# with it — so retiring a construct *properly* (delete the bundle, mark the QA
# doc superseded) turned that document's own commands into `Unknown workflow ID`.
# Measured at FR-149: deleting 19 bundles removed 22 workflow IDs from the
# fixture set, and exactly one was still named by a QA document — the one being
# superseded in the same commit.
#
# An exemption shaped like a subtree absorbs instances that do not exist yet and
# never produces a line in any log (§4.4 shape 8), so this one is required to
# have three properties, and scripts/qa/test-qa-doc-lint-workflow-scope.sh holds
# the negative fixture for each:
#
#   1. Derived from the repository, never from a list. The set comes from each
#      document's own frontmatter, through the real YAML parser already in
#      scripts/qa/doc-lifecycle.rb — not from the committed index, which would
#      add a staleness dependency on a different gate.
#   2. Fails CLOSED, and loudly. If the derivation cannot run, or its output
#      cannot be parsed, or a document is simply absent from it, that document
#      is treated as active and checked — and the check fails. A scope that
#      derives nothing is a failure, not a silent pass (§4.4 shapes 5 and 9: a
#      scope predicate is an assertion and deserves the same attack).
#   3. Visible. The exempt set is printed on every run, including when it is
#      empty. A list that stops growing at least looks suspicious; a silent one
#      does not.
#
# Runs against the current working directory, which the caller has already set
# to the root it means to check. bash 3.2 clean: no mapfile, no associative
# arrays, no namerefs.

# qa_doc_workflow_ids_check
#
# Prints its diagnostics on stdout, its scope failures on stderr, and returns 0
# when every checked document is clean, 1 otherwise.
qa_doc_workflow_ids_check() {
  local prefix="${1:-[qa-doc-lint]}"
  # `rc`, not `status`: this file is sourced, and `status` is a read-only
  # special variable in zsh, where the function would die on its own first line.
  local rc=0

  local superseded_docs="" lifecycle_index=""
  if lifecycle_index=$(ruby scripts/qa/doc-lifecycle.rb --emit-index 2>&1); then
    if ! superseded_docs=$(printf '%s' "$lifecycle_index" | ruby -rjson -e '
          JSON.parse($stdin.read).fetch("documents").each do |path, entry|
            next unless entry["lifecycle"] == "superseded"
            puts path if path.start_with?("docs/qa/orchestrator/")
          end
        ' 2>/dev/null); then
      echo "$prefix ERROR: the doc lifecycle index would not parse; every document will be checked" >&2
      superseded_docs=""
      rc=1
    fi
  else
    echo "$prefix ERROR: doc-lifecycle.rb --emit-index failed; every document will be checked" >&2
    printf '%s\n' "$lifecycle_index" >&2
    superseded_docs=""
    rc=1
  fi

  if [[ -n "$superseded_docs" ]]; then
    echo "$prefix   exempt (lifecycle: superseded):"
    while IFS= read -r skipped; do
      [[ -n "$skipped" ]] && echo "$prefix     $skipped"
    done <<< "$superseded_docs"
  else
    echo "$prefix   exempt (lifecycle: superseded): none — every document is checked"
  fi

  local fixture_workflows
  fixture_workflows=$(rg -A3 'kind: Workflow' fixtures/manifests/bundles/*.yaml 2>/dev/null \
    | rg 'name:' | sed 's/.*name: //' | sort -u)

  local file line match wf_id
  while IFS=: read -r file line match; do
    # A document absent from the exempt set — including one absent from the
    # index entirely, or one whose frontmatter would not parse — is active.
    if [[ -n "$superseded_docs" ]] && rg -qxF "$file" <<< "$superseded_docs"; then
      continue
    fi
    wf_id=$(printf '%s' "$match" | rg -o '\-\-workflow\s+(\S+)' -r '$1')
    # Skip placeholders (<...>), shell variables ($...), and quoted vars ("$...")
    [[ -z "$wf_id" || "$wf_id" == *'<'* || "$wf_id" == *'$'* || "$wf_id" == *'"'* ]] && continue
    if ! rg -qx "$wf_id" <<< "$fixture_workflows"; then
      echo "$prefix Unknown workflow ID '$wf_id' at ${file}:${line} (not in any fixture)"
      rc=1
    fi
  done < <(rg -n -- '--workflow\s+\S+' docs/qa/orchestrator -g '*.md' 2>/dev/null || true)

  return "$rc"
}
