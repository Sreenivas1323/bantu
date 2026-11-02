#!/bin/sh
# A stand-in for `codex exec --json` that costs nothing (see fake-claude.sh).
#
#   BANTU_CODEX_BIN=$PWD/scripts/fake-codex.sh cargo run -p bantud
#
# Set BANTU_FAKE_CODEX_LOG to record each invocation's arguments and the
# first line of its prompt.
prompt=$(cat)
{ printf '%s ' "$@"; printf '| %s\n' "$(printf '%s' "$prompt" | head -n 1)"; } >> "${BANTU_FAKE_CODEX_LOG:-/dev/null}"
last=$(printf '%s' "$prompt" | tail -n 1)
echo '{"type":"thread.started","thread_id":"th_fake"}'
echo '{"type":"turn.started"}'
echo '{"type":"item.started","item":{"id":"item_1","type":"command_execution","command":"ls","status":"in_progress"}}'
echo '{"type":"item.completed","item":{"id":"item_1","type":"command_execution","command":"ls","aggregated_output":"README.md","exit_code":0,"status":"completed"}}'
printf '{"type":"item.completed","item":{"id":"item_2","type":"agent_message","text":"codex: %s"}}\n' "$last"
echo '{"type":"turn.completed","usage":{"input_tokens":10,"cached_input_tokens":0,"output_tokens":4}}'
