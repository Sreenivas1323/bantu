#!/bin/sh
# A stand-in for Claude Code that costs nothing. It speaks the same
# stream-json the daemon parses, so you can develop the UI offline:
#
#   BANTU_CLAUDE_BIN=$PWD/scripts/fake-claude.sh cargo run -p bantud
#
# Prompts containing "slow" take 3 s, "edit" changes files, and "permission"
# asks Bantu for approval through the PermissionRequest hook.
# Set BANTU_FAKE_CLAUDE_LOG to record each invocation's arguments.
{ printf '%s ' "$@" | tr '\n' ' '; echo; } >> "${BANTU_FAKE_CLAUDE_LOG:-/dev/null}"
settings=""; prev=""
for a in "$@"; do [ "$prev" = "--settings" ] && settings="$a"; prev="$a"; done
prompt=$(cat)
case "$prompt" in *permission*)
  url=$(printf '%s' "$settings" | sed -E 's/.*"url":"([^"]+)".*/\1/')
  resp=$(curl -s -X POST -H "Authorization: Bearer $BANTU_HOOK_TOKEN" -H 'content-type: application/json' \
    -d '{"tool_name":"Bash","tool_input":{"command":"rm -rf build"}}' "$url")
  case "$resp" in *'"allow"'*) prompt="decision allow" ;; *) prompt="decision deny" ;; esac ;;
esac
case "$prompt" in *slow*) sleep 3 ;; esac
case "$prompt" in *edit*) echo agent-was-here >> README.md; echo new > agent-file.txt ;; esac
echo '{"type":"system","subtype":"init"}'
echo '{"type":"assistant","message":{"content":[{"type":"tool_use","id":"t1","name":"Bash","input":{"command":"ls"}}]}}'
echo '{"type":"user","message":{"content":[{"type":"tool_result","tool_use_id":"t1","content":"README.md","is_error":false}]}}'
echo '{"type":"stream_event","parent_tool_use_id":null,"event":{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"ech"}}}'
echo '{"type":"stream_event","parent_tool_use_id":null,"event":{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"o: "}}}'
printf '{"type":"assistant","message":{"content":[{"type":"text","text":"echo: %s"}]}}\n' "$prompt"
echo '{"type":"result","is_error":false,"duration_ms":5,"total_cost_usd":0.25,"usage":{"input_tokens":3,"output_tokens":2}}'
