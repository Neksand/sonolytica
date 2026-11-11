#!/usr/bin/env bash
# Build a clean directory for one agent benchmark session.
#
#   scripts/bench/prepare_agent_fixture.sh --name hard \
#       --media /private/tmp/bench-hard/meeting.mp4
#
# The directory gets exactly four things: the media, a copy of the skill, the
# full-metal binary and PROMPT.md. No previous documents, no repository, no
# sibling run — reading a neighbour's JSON is how benchmark results get faked.
#
# The session itself is not automated: start it by hand in that directory and
# read its tokens with `session_tokens.sh <session-id>`.

set -euo pipefail

repo=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)
name=""
media=""
dest=""
prompt=""
binary=""
skill=""
force=0
build=1

usage() {
    sed -n '2,12p' "$0" | sed 's/^# \{0,1\}//'
    cat <<'EOF'

Options:
  --name NAME     fixture name; also picks scripts/bench/prompts/NAME.md
  --media PATH    the media file the session is allowed to see
  --dest PATH     destination (default: /private/tmp/bench-agent-NAME)
  --prompt PATH   override the prompt file
  --skill PATH    skill directory to copy (default: skills/sonolytica)
  --binary PATH   override the CLI (default: target/debug/sonolytica)
  --no-build      fail instead of running cargo when the binary is missing
  --force         wipe an existing destination
EOF
    exit 1
}

while (($#)); do
    case "$1" in
        --name) name=${2:?}; shift 2 ;;
        --media) media=${2:?}; shift 2 ;;
        --dest) dest=${2:?}; shift 2 ;;
        --prompt) prompt=${2:?}; shift 2 ;;
        --skill) skill=${2:?}; shift 2 ;;
        --binary) binary=${2:?}; shift 2 ;;
        --no-build) build=0; shift ;;
        --force) force=1; shift ;;
        -h|--help) usage ;;
        *) echo "unknown argument: $1" >&2; usage ;;
    esac
done

[[ -n $name && -n $media ]] || usage
[[ -f $media ]] || { echo "no such media: $media" >&2; exit 1; }
prompt=${prompt:-$repo/scripts/bench/prompts/$name.md}
[[ -f $prompt ]] || { echo "no such prompt: $prompt" >&2; exit 1; }
skill=${skill:-$repo/skills/sonolytica}
[[ -f $skill/SKILL.md ]] || { echo "no such skill: $skill" >&2; exit 1; }
dest=${dest:-/private/tmp/bench-agent-$name}
binary=${binary:-$repo/target/debug/sonolytica}

if [[ -e $dest ]]; then
    if ((!force)); then
        echo "destination exists: $dest (use --force to wipe it)" >&2
        exit 1
    fi
    rm -rf "$dest"
fi
mkdir -p "$dest"

if [[ ! -x $binary ]]; then
    ((build)) || { echo "missing binary: $binary" >&2; exit 1; }
    (cd "$repo" && cargo build -p sonolytica-cli --features full-metal)
fi

cp "$media" "$dest/"
cp -R "$skill" "$dest/skills-sonolytica"
cp "$binary" "$dest/sonolytica"
cp "$prompt" "$dest/PROMPT.md"

cat <<EOF
Fixture ready: $dest
  media        $(basename "$media")
  skill        skills-sonolytica/SKILL.md
  binary       ./sonolytica ($("$dest/sonolytica" --version 2>/dev/null || echo "version unknown"))
  prompt       PROMPT.md

Start one session in $dest, nothing else. The prompt follows.

---- PROMPT.md ----
EOF
cat "$dest/PROMPT.md"
