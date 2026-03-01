#!/bin/bash
set -euo pipefail

REPO="shonenada/codex-session"
BINARY_NAME="codex-session"
EMPTY_SHA="e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"

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

# Fetch SHA256 for each target
ARM64_URL="https://github.com/${REPO}/releases/download/${TAG}/${BINARY_NAME}-darwin-arm64.tar.gz"
X86_64_URL="https://github.com/${REPO}/releases/download/${TAG}/${BINARY_NAME}-darwin-x86_64.tar.gz"
LINUX_URL="https://github.com/${REPO}/releases/download/${TAG}/${BINARY_NAME}-linux-x86_64.tar.gz"

ARM64_SHA=$(curl -sL "${ARM64_URL}" | shasum -a 256 | awk '{print $1}')
X86_64_SHA=$(curl -sL "${X86_64_URL}" | shasum -a 256 | awk '{print $1}')
LINUX_SHA=$(curl -sL "${LINUX_URL}" | shasum -a 256 | awk '{print $1}')

if [ -z "$ARM64_SHA" ] || [ "$ARM64_SHA" = "$EMPTY_SHA" ]; then
    echo "Error: Could not fetch darwin-arm64 release asset or asset is empty." >&2
    exit 1
fi

if [ -z "$X86_64_SHA" ] || [ "$X86_64_SHA" = "$EMPTY_SHA" ]; then
    echo "Error: Could not fetch darwin-x86_64 release asset or asset is empty." >&2
    exit 1
fi

if [ -z "$LINUX_SHA" ] || [ "$LINUX_SHA" = "$EMPTY_SHA" ]; then
    echo "Error: Could not fetch linux-x86_64 release asset or asset is empty." >&2
    exit 1
fi

cat <<EOF
class CodexSession < Formula
  desc "TUI session manager for OpenAI Codex CLI"
  homepage "https://github.com/${REPO}"
  version "${VERSION}"

  on_macos do
    on_arm do
      url "https://github.com/${REPO}/releases/download/v#{version}/${BINARY_NAME}-darwin-arm64.tar.gz"
      sha256 "${ARM64_SHA}"
    end
    on_intel do
      url "https://github.com/${REPO}/releases/download/v#{version}/${BINARY_NAME}-darwin-x86_64.tar.gz"
      sha256 "${X86_64_SHA}"
    end
  end

  on_linux do
    on_intel do
      url "https://github.com/${REPO}/releases/download/v#{version}/${BINARY_NAME}-linux-x86_64.tar.gz"
      sha256 "${LINUX_SHA}"
    end
  end

  def install
    bin.install "${BINARY_NAME}"
  end

  test do
    system "#{bin}/${BINARY_NAME}", "--version"
  end
end
EOF
