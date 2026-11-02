#!/bin/sh
# Install Bantu: the bantud daemon plus its web app, then run it as a
# per-user service (launchd on macOS, systemd on Linux).
#
#   curl -fsSL https://raw.githubusercontent.com/Sreenivas1323/bantu/main/deploy/install.sh | sh
#
# Options (environment variables):
#   BANTU_VERSION=v0.1.0   install a specific release (default: latest)
#   BANTU_PREFIX=~/.local  where bin/ and share/ go
#   BANTU_NO_SERVICE=1     don't register the background service
#   BANTU_DOWNLOAD_BASE=…  fetch archives from a mirror (or file:// for testing)
set -eu

REPO="Sreenivas1323/bantu"
VERSION="${BANTU_VERSION:-latest}"
PREFIX="${BANTU_PREFIX:-$HOME/.local}"

say() { printf '%s\n' "$*"; }
die() { say "error: $*" >&2; exit 1; }

case "$(uname -s)-$(uname -m)" in
  Darwin-arm64) target=aarch64-apple-darwin ;;
  Darwin-x86_64) target=x86_64-apple-darwin ;;
  Linux-x86_64) target=x86_64-unknown-linux-gnu ;;
  Linux-aarch64 | Linux-arm64) target=aarch64-unknown-linux-gnu ;;
  *) die "no prebuilt Bantu for $(uname -s) $(uname -m); build from source: cargo install --git https://github.com/$REPO bantud" ;;
esac

command -v curl >/dev/null || die "curl is required"
command -v tar >/dev/null || die "tar is required"
command -v git >/dev/null || say "warning: git isn't installed; Bantu needs it to create workspaces"

if [ -n "${BANTU_DOWNLOAD_BASE:-}" ]; then
  base="$BANTU_DOWNLOAD_BASE"
elif [ "$VERSION" = latest ]; then
  base="https://github.com/$REPO/releases/latest/download"
else
  base="https://github.com/$REPO/releases/download/$VERSION"
fi
archive="bantu-$target.tar.gz"

tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT INT TERM

say "Downloading $archive ($VERSION)…"
curl -fsSL "$base/$archive" -o "$tmp/$archive" || die "download failed: $base/$archive"
curl -fsSL "$base/SHA256SUMS" -o "$tmp/SHA256SUMS" || die "download failed: $base/SHA256SUMS"

expected="$(grep " $archive\$" "$tmp/SHA256SUMS" | cut -d' ' -f1)"
[ -n "$expected" ] || die "$archive is missing from SHA256SUMS"
if command -v sha256sum >/dev/null; then
  actual="$(sha256sum "$tmp/$archive" | cut -d' ' -f1)"
else
  actual="$(shasum -a 256 "$tmp/$archive" | cut -d' ' -f1)"
fi
[ "$expected" = "$actual" ] || die "checksum mismatch for $archive"

tar -xzf "$tmp/$archive" -C "$tmp"
src="$tmp/bantu-$target"
mkdir -p "$PREFIX/bin" "$PREFIX/share/bantu"
cp "$src/bin/bantud" "$PREFIX/bin/bantud.new"
chmod 755 "$PREFIX/bin/bantud.new"
mv -f "$PREFIX/bin/bantud.new" "$PREFIX/bin/bantud"
rm -rf "$PREFIX/share/bantu/web"
cp -R "$src/share/bantu/web" "$PREFIX/share/bantu/web"
say "Installed $PREFIX/bin/bantud"

if [ "${BANTU_NO_SERVICE:-}" != 1 ]; then
  "$PREFIX/bin/bantud" service install
fi

case ":$PATH:" in
  *":$PREFIX/bin:"*) ;;
  *) say "Add $PREFIX/bin to your PATH to run \`bantud\` directly." ;;
esac
command -v claude >/dev/null || say "Install Claude Code (https://claude.com/claude-code) and sign in; Bantu runs your own \`claude\`."

say ""
say "Open Bantu: $("$PREFIX/bin/bantud" url)"
