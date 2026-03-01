#!/bin/bash
set -euo pipefail

REPO="shonenada/codex-session"
BINARY_NAME="codex-session"

usage() {
    echo "Usage: $0 <tag>"
    echo "Example: $0 v0.1.0"
    exit 1
}

if [ $# -ne 1 ]; then
    usage
fi

TAG="$1"
VERSION="${TAG#v}"

DOWNLOAD_URL="https://github.com/${REPO}/releases/download/${TAG}/${BINARY_NAME}-darwin-arm64.tar.gz"

SHA256=$(curl -sL "${DOWNLOAD_URL}" | shasum -a 256 | awk '{print $1}')

if [ -z "$SHA256" ] || [ "$SHA256" = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855" ]; then
    echo "Error: Could not fetch release asset or asset is empty." >&2
    echo "Make sure the release ${TAG} exists and contains ${BINARY_NAME}-darwin-arm64.tar.gz" >&2
    exit 1
fi

cat <<EOF
class CodexSession < Formula
  desc "TUI session manager for OpenAI Codex CLI"
  homepage "https://github.com/${REPO}"
  version "${VERSION}"
  sha256 "${SHA256}"

  url "https://github.com/${REPO}/releases/download/v#{version}/${BINARY_NAME}-darwin-arm64.tar.gz"

  def install
    bin.install "${BINARY_NAME}"
  end

  test do
    system "#{bin}/${BINARY_NAME}", "--version"
  end
end
EOF
