#!/bin/sh
# Install email-cli — downloads pre-built binary from GitHub Releases.
# Usage: curl -fsSL https://raw.githubusercontent.com/199-biotechnologies/email-cli/main/install.sh | sh
set -e

REPO="199-biotechnologies/email-cli"
INSTALL_DIR="${INSTALL_DIR:-/usr/local/bin}"

ARCH=$(uname -m)
case "$ARCH" in
  x86_64|amd64) PLATFORM="x86_64-darwin" ;;
  arm64|aarch64) PLATFORM="arm64-darwin" ;;
  *) echo "Unsupported architecture: $ARCH" >&2; exit 1 ;;
esac

VERSION=$(curl -fsSL "https://api.github.com/repos/${REPO}/releases/latest" | grep '"tag_name"' | sed 's/.*"tag_name": *"//;s/".*//')
if [ -z "$VERSION" ]; then
  echo "Failed to fetch latest version" >&2
  exit 1
fi

TARBALL="email-cli-${VERSION}-${PLATFORM}.tar.gz"
URL="https://github.com/${REPO}/releases/download/${VERSION}/${TARBALL}"

echo "Installing email-cli ${VERSION} (${PLATFORM})..."

TMPDIR=$(mktemp -d)
trap 'rm -rf "$TMPDIR"' EXIT

curl -fsSL "$URL" -o "${TMPDIR}/${TARBALL}"
tar xzf "${TMPDIR}/${TARBALL}" -C "$TMPDIR"

if [ -w "$INSTALL_DIR" ]; then
  mv "${TMPDIR}/email-cli" "${INSTALL_DIR}/email-cli"
else
  sudo mv "${TMPDIR}/email-cli" "${INSTALL_DIR}/email-cli"
fi

echo "Installed email-cli ${VERSION} to ${INSTALL_DIR}/email-cli"
echo ""
echo "Get started:"
echo "  email-cli profile add default --api-key-env RESEND_API_KEY"
echo "  email-cli agent-info"
