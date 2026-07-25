#!/usr/bin/env bash

set -euo pipefail

MCP_CONFIG=""
RESUME_TOKEN=""
ARGS="$*"
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
SESSION_ID="${RESUME_TOKEN:-00000000-0000-4000-8000-000000000126}"
TRACE_PATH="${FR126_FAKE_TRACE:-$PWD/.fr126-fake-trace}"
printf '%s\t%s\n' "${RESUME_TOKEN:+resume}" "$ARGS" >> "$TRACE_PATH"

jq -nc --arg session "$SESSION_ID" \
  '{type:"system",subtype:"init",session_id:$session}'

if [[ -z "$MCP_CONFIG" ]]; then
  echo "fake claude requires --mcp-config" >&2
  exit 2
fi

SHIM="$(jq -r '.mcpServers.orch.command' "$MCP_CONFIG")"
CALLBACK_URL="$(jq -r '.mcpServers.orch.env.ORCH_MCP_CALLBACK_URL' "$MCP_CONFIG")"
CALLBACK_TOKEN="$(jq -r '.mcpServers.orch.env.ORCH_MCP_CALLBACK_TOKEN' "$MCP_CONFIG")"
TOOL_ID="fr126-mark-done"
ARGUMENTS='{"summary":"FR-126 typed Claude parity completed"}'

jq -nc --arg id "$TOOL_ID" --argjson args "$ARGUMENTS" \
  '{type:"assistant",message:{content:[{type:"tool_use",id:$id,name:"mcp__orch__mark_done",input:$args}]}}'

REQUEST="$(jq -nc --arg id "$TOOL_ID" --argjson args "$ARGUMENTS" \
  '{jsonrpc:"2.0",id:$id,method:"tools/call",params:{name:"mark_done",arguments:$args}}')"
RESPONSE="$(
  printf '%s\n' "$REQUEST" |
    env ORCH_MCP_CALLBACK_URL="$CALLBACK_URL" \
        ORCH_MCP_CALLBACK_TOKEN="$CALLBACK_TOKEN" \
        "$SHIM"
)"
PAYLOAD="$(jq -c '.result.structuredContent // .error' <<<"$RESPONSE")"
IS_ERROR="$(jq -r '.result.isError // (.error != null)' <<<"$RESPONSE")"
CONTENT="$(jq -nc --arg text "$PAYLOAD" '[{type:"text",text:$text}]')"
jq -nc --arg id "$TOOL_ID" --argjson content "$CONTENT" --argjson is_error "$IS_ERROR" \
  '{type:"user",message:{content:[{type:"tool_result",tool_use_id:$id,content:$content,is_error:$is_error}]}}'

jq -nc --arg session "$SESSION_ID" \
  '{type:"result",subtype:"success",is_error:false,result:"FR-126 fake Claude complete",total_cost_usd:0,num_turns:1,session_id:$session}'
