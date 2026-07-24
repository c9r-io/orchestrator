#!/usr/bin/env bash

set -euo pipefail

ARGS="$*"
MCP_CONFIG=""
RESUME_TOKEN=""
while [[ "$#" -gt 0 ]]; do
  case "$1" in
    --mcp-config)
      MCP_CONFIG="${2:-}"
      shift 2
      ;;
    --resume)
      RESUME_TOKEN="${2:-}"
      shift 2
      ;;
    *)
      shift
      ;;
  esac
done

INITIAL_INPUT=""
IFS= read -r INITIAL_INPUT || true
PROMPT="$(jq -r '.message.content // ""' <<<"$INITIAL_INPUT" 2>/dev/null || true)"
ARGS="$ARGS $PROMPT"
SESSION_ID="${RESUME_TOKEN:-00000000-0000-4000-8000-000000000124}"
TRACE_PATH="${FR124_FAKE_TRACE:-$PWD/.fr124-fake-trace}"
TRACE_STATE="fresh"
[[ -n "$RESUME_TOKEN" ]] && TRACE_STATE="resume"
printf '%s\t%s\n' "$TRACE_STATE" "$ARGS" >> "$TRACE_PATH"

jq -nc --arg session "$SESSION_ID" \
  '{type:"system",subtype:"init",session_id:$session}'

if [[ -n "$MCP_CONFIG" ]]; then
  SHIM="$(jq -r '.mcpServers.orch.command' "$MCP_CONFIG")"
  CALLBACK_URL="$(jq -r '.mcpServers.orch.env.ORCH_MCP_CALLBACK_URL' "$MCP_CONFIG")"
  CALLBACK_TOKEN="$(jq -r '.mcpServers.orch.env.ORCH_MCP_CALLBACK_TOKEN' "$MCP_CONFIG")"

  rpc() {
    printf '%s\n' "$1" | \
      env ORCH_MCP_CALLBACK_URL="$CALLBACK_URL" \
          ORCH_MCP_CALLBACK_TOKEN="$CALLBACK_TOKEN" \
          "$SHIM"
  }

  tool_call() {
    local id="$1"
    local name="$2"
    local arguments="$3"
    local request response payload is_error content
    jq -nc --arg id "$id" --arg name "$name" --argjson args "$arguments" \
      '{type:"assistant",message:{content:[{type:"tool_use",id:$id,name:("mcp__orch__"+$name),input:$args}]}}'
    request="$(jq -nc --arg id "$id" --arg name "$name" --argjson args "$arguments" \
      '{jsonrpc:"2.0",id:$id,method:"tools/call",params:{name:$name,arguments:$args}}')"
    response="$(rpc "$request")"
    payload="$(jq -c '.result.structuredContent // .error' <<<"$response")"
    is_error="$(jq -r '.result.isError // (.error != null)' <<<"$response")"
    content="$(jq -nc --arg text "$payload" '[{type:"text",text:$text}]')"
    jq -nc --arg id "$id" --argjson content "$content" --argjson is_error "$is_error" \
      '{type:"user",message:{content:[{type:"tool_result",tool_use_id:$id,content:$content,is_error:$is_error}]}}'
  }

  if [[ "$ARGS" == *"CALL_GENERATE_QA_ITEMS"* ]]; then
    tool_call "generate-qa" "generate_items" \
      '{"replace":true,"items":[{"id":"docs/qa/pilot.md","label":"docs/qa/pilot.md"}]}'
  elif [[ "$ARGS" == *"CALL_GENERATE_PROMOTION_ITEMS"* ]]; then
    tool_call "generate-promotion" "generate_items" \
      '{"replace":true,"items":[{"id":"devto","label":"Dev.to","vars":{"content_type":"blog_post","api_publishable":"false","platform_priority":"5","promotion_summary":"typed coordination parity"}}]}'
  elif [[ "$ARGS" == *"CALL_GENERATE_EVOLUTION_ITEMS"* ]]; then
    tool_call "generate-evolution" "generate_items" \
      '{"replace":true,"items":[{"id":"approach-a","label":"Approach A","vars":{"approach":"minimal","strategy":"small change"}},{"id":"approach-b","label":"Approach B","vars":{"approach":"thorough","strategy":"full validation"}}]}'
  fi
  if [[ "$ARGS" == *"CALL_RUN_TESTS"* ]]; then
    tool_call "run-tests" "run_tests" '{"target":"workspace"}'
  fi
  if [[ "$ARGS" == *"CALL_SCAN_TICKETS"* ]]; then
    tool_call "scan-tickets" "scan_tickets" '{}'
  fi
  if [[ "$ARGS" == *"CALL_RECORD_METRIC"* ]]; then
    tool_call "record-metric" "record_metric" '{"name":"score","value":91}'
  fi
  if [[ "$ARGS" == *"CALL_MARK_QA_PASSED"* ]]; then
    tool_call "mark-qa" "mark_item" \
      '{"status":"qa_passed","summary":"typed QA parity passed"}'
  elif [[ "$ARGS" == *"CALL_MARK_VERIFIED"* ]]; then
    tool_call "mark-verified" "mark_item" \
      '{"status":"verified","summary":"typed coordination parity passed"}'
  fi
fi

jq -nc --arg session "$SESSION_ID" \
  '{type:"result",subtype:"success",is_error:false,result:"FR-124 fake driver complete",total_cost_usd:0,num_turns:1,session_id:$session}'
