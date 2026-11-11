#!/usr/bin/env bash
# Token totals for one agent benchmark session, straight from OpenCode's store.
#
#   scripts/bench/session_tokens.sh ses_abc123
#   scripts/bench/session_tokens.sh --last          # most recent session
#
# Fresh = tokens_input + tokens_output; that is the number the benchmark
# reports. Reasoning and cache reads are printed for context, not summed in.

set -euo pipefail

db=${OPENCODE_DB:-$HOME/.local/share/opencode/opencode.db}
[[ -f $db ]] || { echo "no OpenCode database at $db" >&2; exit 1; }
command -v sqlite3 >/dev/null || { echo "sqlite3 is required" >&2; exit 1; }

if [[ ${1:-} == "--last" ]]; then
    id=$(sqlite3 -readonly "$db" \
        "select id from session_v2 where parent_id is null order by time_updated desc limit 1")
elif [[ -n ${1:-} ]]; then
    id=$1
else
    sed -n '2,9p' "$0" | sed 's/^# \{0,1\}//'
    exit 1
fi

sqlite3 -readonly -separator $'\t' "$db" "
    select coalesce(title, ''), model, coalesce(agent, ''), directory,
           tokens_input, tokens_output, tokens_reasoning,
           tokens_cache_read, tokens_cache_write, cost
    from session_v2 where id = '$id';
" | while IFS=$'\t' read -r title model agent directory input output reasoning \
    cache_read cache_write cost; do
    printf 'session    %s\n' "$id"
    printf 'title      %s\n' "${title:-—}"
    printf 'model      %s\n' "${model:-—}"
    printf 'agent      %s\n' "${agent:-—}"
    printf 'directory  %s\n' "${directory:-—}"
    printf 'input      %s\n' "$input"
    printf 'output     %s\n' "$output"
    printf 'reasoning  %s\n' "$reasoning"
    printf 'cache read %s\n' "$cache_read"
    printf 'cache write %s\n' "${cache_write:-0}"
    printf 'cost       %s\n' "$cost"
    printf 'fresh      %s\n' "$((input + output))"
done

count=$(sqlite3 -readonly "$db" "select count(*) from session_v2 where id = '$id';")
((count)) || { echo "unknown session: $id" >&2; exit 1; }
