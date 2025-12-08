#!/usr/bin/env bash
set -euo pipefail

REPO="shonenada/codex-session"
BIN_NAME="codex-session"
VERSION="${VERSION:-latest}"
INSTALL_DIR="${INSTALL_DIR:-/usr/local/bin}"

command -v curl >/dev/null 2>&1 || { echo "curl is required" >&2; exit 1; }
command -v python3 >/dev/null 2>&1 || { echo "python3 is required" >&2; exit 1; }

OS=$(uname -s)
case "$OS" in
  Linux) OS_NAME="linux" ;;
  Darwin) OS_NAME="macos" ;;
  *) echo "Unsupported OS: $OS" >&2; exit 1 ;;
esac

ARCH=$(uname -m)
ASSET_NAME="${BIN_NAME}-${OS_NAME}-${ARCH}"

API_URL="https://api.github.com/repos/${REPO}/releases/${VERSION}"
if [[ "$VERSION" == "latest" ]]; then
  API_URL="https://api.github.com/repos/${REPO}/releases/latest"
else
  API_URL="https://api.github.com/repos/${REPO}/releases/tags/${VERSION}"
fi

echo "Fetching release metadata from ${API_URL}..."
DOWNLOAD_URL=$(python3 - "${API_URL}" "${ASSET_NAME}" <<'PY'
import json
import sys
import urllib.request

if len(sys.argv) < 3:
    sys.exit("Usage: script <api> <asset_name>")

api = sys.argv[1]
asset_name = sys.argv[2]

with urllib.request.urlopen(api) as resp:
    data = json.load(resp)

assets = data.get("assets", [])
if not assets:
    sys.exit("No assets found in release metadata")

for asset in assets:
    if asset.get("name") == asset_name:
        print(asset["browser_download_url"])
        sys.exit(0)

available = [a.get("name") for a in assets if a.get("name")]
sys.exit(f"Asset {asset_name} not found. Available assets: {available}")
PY
)

TMP_DIR=$(mktemp -d)
trap 'rm -rf "${TMP_DIR}"' EXIT

echo "Downloading ${ASSET_NAME}..."
curl -fsL "${DOWNLOAD_URL}" -o "${TMP_DIR}/${ASSET_NAME}"

mkdir -p "${INSTALL_DIR}"
echo "Installing to ${INSTALL_DIR}/${BIN_NAME} (may require sudo)..."
install -m 0755 "${TMP_DIR}/${ASSET_NAME}" "${INSTALL_DIR}/${BIN_NAME}"

echo "Installed ${BIN_NAME} -> ${INSTALL_DIR}/${BIN_NAME}"
