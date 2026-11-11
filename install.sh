#!/usr/bin/env bash
# Install a SONOLYTICA release binary.
#
#   curl -fsSL https://raw.githubusercontent.com/neksand/sonolytica/main/install.sh | sh
#
# Environment:
#   SONOLYTICA_REPO     GitHub owner/name (default: neksand/sonolytica)
#   SONOLYTICA_VERSION  tag to install (default: latest release)
#   SONOLYTICA_BIN_DIR  install directory (default: ~/.local/bin)
#
# The script downloads the release tarball, verifies its SHA-256, and never
# touches anything else. Re-run it to upgrade.

set -euo pipefail

repo=${SONOLYTICA_REPO:-neksand/sonolytica}
version=${SONOLYTICA_VERSION:-latest}
bin_dir=${SONOLYTICA_BIN_DIR:-$HOME/.local/bin}

case "$(uname -s)" in
    Darwin) os=apple-darwin ;;
    Linux) os=unknown-linux-gnu ;;
    *) echo "unsupported OS: $(uname -s); build from source instead" >&2; exit 1 ;;
esac
case "$(uname -m)" in
    arm64 | aarch64) arch=aarch64 ;;
    x86_64 | amd64) arch=x86_64 ;;
    *) echo "unsupported architecture: $(uname -m)" >&2; exit 1 ;;
esac
target="${arch}-${os}"

if [[ $version == latest ]]; then
    url="https://github.com/${repo}/releases/latest/download"
else
    url="https://github.com/${repo}/releases/download/${version}"
fi
archive="sonolytica-${target}.tar.gz"

tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT

echo "downloading ${archive} from ${repo}"
curl -fsSL "${url}/${archive}" -o "${tmp}/${archive}"
curl -fsSL "${url}/${archive}.sha256" -o "${tmp}/${archive}.sha256"

expected=$(awk '{print $1}' "${tmp}/${archive}.sha256")
if command -v sha256sum >/dev/null 2>&1; then
    actual=$(sha256sum "${tmp}/${archive}" | awk '{print $1}')
else
    actual=$(shasum -a 256 "${tmp}/${archive}" | awk '{print $1}')
fi
if [[ $expected != "$actual" ]]; then
    echo "checksum mismatch: expected $expected, got $actual" >&2
    exit 1
fi

tar xzf "${tmp}/${archive}" -C "$tmp"
mkdir -p "$bin_dir"
install -m 0755 "${tmp}/sonolytica" "$bin_dir/sonolytica"
install -m 0755 "${tmp}/sonolytica-eval" "$bin_dir/sonolytica-eval"

echo "installed to ${bin_dir}"
case ":$PATH:" in
    *":${bin_dir}:"*) ;;
    *) echo "add ${bin_dir} to PATH: export PATH=\"${bin_dir}:\$PATH\"" ;;
esac
echo "next: sonolytica doctor && sonolytica install-skill && sonolytica models fetch --set core"
