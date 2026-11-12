#!/usr/bin/env bash
# One-file demo: analyze, brief, report.
#
#   scripts/demo.sh talk.mp4
#   SONOLYTICA_BIN=./target/debug/sonolytica scripts/demo.sh talk.mp4
#
# Run `sonolytica setup` first; the demo does not download models.

set -euo pipefail

bin=${SONOLYTICA_BIN:-sonolytica}
media=${1:?usage: scripts/demo.sh MEDIA}
out=${2:-${media%.*}.sonolytica.json}
html=${out%.json}.html

"$bin" doctor >/dev/null
"$bin" analyze "$media" --out "$out" --profile standard --ocr --caption --summary
"$bin" query brief "$out"
"$bin" report "$out" --out "$html"

echo
echo "document: $out"
echo "report:   $html"
echo "next:     $bin mcp   # Claude Desktop / Cursor / IDE agents"
