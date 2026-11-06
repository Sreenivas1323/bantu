#!/bin/sh
# A stand-in for an ACP agent (OpenCode, Gemini CLI, Cursor) that costs nothing.
# It speaks just enough JSON-RPC over stdio for the daemon's ACP driver:
#
#   BANTU_OPENCODE_BIN=$PWD/scripts/fake-acp.sh cargo run -p bantud
#
# Prompts containing "permission" ask for approval first, and "edit" changes a
# file. Set BANTU_FAKE_ACP_LOG to record each message the daemon sends.
case "$1" in
  --version) echo "9.9.9"; exit 0 ;;
esac
log="${BANTU_FAKE_ACP_LOG:-/dev/null}"
while IFS= read -r line; do
  printf '%s\n' "$line" >> "$log"
  id=$(printf '%s' "$line" | sed -nE 's/.*"id":([0-9]+).*/\1/p')
  case "$line" in
    *'"method":"initialize"'*)
      printf '{"jsonrpc":"2.0","id":%s,"result":{"protocolVersion":1,"agentCapabilities":{"loadSession":true,"mcpCapabilities":{"http":true}}}}\n' "$id" ;;
    *'"method":"session/new"'*)
      printf '{"jsonrpc":"2.0","id":%s,"result":{"sessionId":"acp_fake","modes":{"currentModeId":"default","availableModes":[{"id":"default"},{"id":"plan"}]}}}\n' "$id" ;;
    *'"method":"session/load"'*)
      # History replay; the daemon must not show this a second time.
      echo '{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"acp_fake","update":{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"old history"}}}}'
      printf '{"jsonrpc":"2.0","id":%s,"result":{}}\n' "$id" ;;
    *'"method":"session/set_mode"'*)
      printf '{"jsonrpc":"2.0","id":%s,"result":{}}\n' "$id" ;;
    *'"method":"session/cancel"'*)
      exit 0 ;;
    *'"method":"session/prompt"'*)
      prompt_id=$id
      prompt=$(printf '%s' "$line" | sed -nE 's/.*"text":"(.*)"\}\].*/\1/p')
      case "$prompt" in *slow*) sleep 30 ;; esac
      case "$prompt" in *edit*) echo acp-was-here >> README.md ;; esac
      case "$prompt" in *permission*)
        echo '{"jsonrpc":"2.0","id":99,"method":"session/request_permission","params":{"sessionId":"acp_fake","toolCall":{"toolCallId":"p1","title":"rm -rf build","kind":"execute","rawInput":{"command":"rm -rf build"}},"options":[{"optionId":"yes","name":"Allow","kind":"allow_once"},{"optionId":"no","name":"Reject","kind":"reject_once"}]}}'
        IFS= read -r answer
        printf '%s\n' "$answer" >> "$log"
        case "$answer" in *'"optionId":"yes"'*) prompt="decision allow" ;; *) prompt="decision deny" ;; esac ;;
      esac
      echo '{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"acp_fake","update":{"sessionUpdate":"tool_call","toolCallId":"c1","title":"ls","kind":"execute","status":"in_progress"}}}'
      echo '{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"acp_fake","update":{"sessionUpdate":"tool_call_update","toolCallId":"c1","status":"completed","content":[{"type":"content","content":{"type":"text","text":"README.md"}}]}}}'
      echo '{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"acp_fake","update":{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"acp: "}}}}'
      printf '{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"acp_fake","update":{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"%s"}}}}\n' "$prompt"
      printf '{"jsonrpc":"2.0","id":%s,"result":{"stopReason":"end_turn"}}\n' "$prompt_id" ;;
  esac
done
