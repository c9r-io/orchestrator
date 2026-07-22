#!/usr/bin/env bash

set -euo pipefail

MCP_CONFIG=""
while [[ "$#" -gt 0 ]]; do
  if [[ "$1" == "--mcp-config" && "$#" -ge 2 ]]; then
    MCP_CONFIG="$2"
    shift 2
    continue
  fi
  shift
done

[[ -n "$MCP_CONFIG" ]] || { echo "missing --mcp-config" >&2; exit 2; }
SHIM="$(jq -r '.mcpServers.orch.command' "$MCP_CONFIG")"
CALLBACK_URL="$(jq -r '.mcpServers.orch.env.ORCH_MCP_CALLBACK_URL' "$MCP_CONFIG")"
CALLBACK_TOKEN="$(jq -r '.mcpServers.orch.env.ORCH_MCP_CALLBACK_TOKEN' "$MCP_CONFIG")"
SESSION_ID="00000000-0000-4000-8000-000000000118"

rpc() {
  local request="$1"
  printf '%s\n' "$request" | \
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
  payload="$(jq -c '.result.structuredContent' <<<"$response")"
  is_error="$(jq -r '.result.isError // false' <<<"$response")"
  content="$(jq -nc --arg text "$payload" '[{type:"text",text:$text}]')"
  jq -nc --arg id "$id" --argjson content "$content" --argjson is_error "$is_error" \
    '{type:"user",message:{content:[{type:"tool_result",tool_use_id:$id,content:$content,is_error:$is_error}]}}'
}

jq -nc --arg session "$SESSION_ID" '{type:"system",subtype:"init",session_id:$session}'
tool_call "tool-run-tests" "run_tests" '{"target":"workspace"}'
tool_call "tool-scan-tickets" "scan_tickets" '{}'
tool_call "tool-mark-item" "mark_item" \
  '{"status":"qa_passed","summary":"isolated coordination pilot passed"}'
jq -nc --arg session "$SESSION_ID" \
  '{type:"result",subtype:"success",is_error:false,result:"coordination pilot complete",total_cost_usd:0,num_turns:1,session_id:$session}'
