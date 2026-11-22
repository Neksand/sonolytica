#!/usr/bin/env bash
# Build the marketplace skill zip. No binary, models or runtime files are
# included: the skill installs into an agent and calls the user's local CLI.
#
#   scripts/package-skill.sh [OUTPUT_DIR]
#
# Output: <OUTPUT_DIR>/sonolytica-skill-<version>.zip

set -euo pipefail

root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
version=$(sed -n 's/^version = "\(.*\)"/\1/p' "$root/Cargo.toml" | head -1)
out_dir=${1:-"$root/dist"}
mkdir -p "$out_dir"
out_dir=$(cd "$out_dir" && pwd)
out="$out_dir/sonolytica-skill-${version}.zip"

tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
cp "$root/skills/sonolytica/SKILL.md" "$tmp/SKILL.md"
cp "$root/skills/sonolytica/README.md" "$tmp/README.md"
cp -R "$root/skills/sonolytica/references" "$tmp/references"
cp "$root/LICENSE-MIT" "$root/LICENSE-APACHE" "$root/NOTICE" "$tmp/"

# Frontmatter is what the marketplace indexes.
head -1 "$tmp/SKILL.md" | grep -qx -- '---'
grep -qx 'name: sonolytica' "$tmp/SKILL.md"
grep -q '^description:' "$tmp/SKILL.md"

(cd "$tmp" && zip -X -q -r "$out" .)

# The package contract: root SKILL.md, references, no runtime payload.
unzip -Z1 "$out" | grep -qx 'SKILL.md'
unzip -Z1 "$out" | grep -q '^references/'
if unzip -Z1 "$out" | grep -Eiq '\.(exe|dll|dylib|so|onnx|ggml|bin|wav|mp4|mov|zip)$'; then
    echo "refusing: runtime or media file in skill zip" >&2
    exit 1
fi
size_kb=$(du -k "$out" | awk '{print $1}')
if (( size_kb > 50 * 1024 )); then
    echo "refusing: ${size_kb} KB exceeds the 50 MB marketplace limit" >&2
    exit 1
fi

(cd "$out_dir" && shasum -a 256 "$(basename "$out")" > "$(basename "$out").sha256")

echo "$out (${size_kb} KB)"
echo "root: SKILL.md; references/; LICENSE-MIT; LICENSE-APACHE; NOTICE"
