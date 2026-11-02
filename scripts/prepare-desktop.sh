#!/bin/sh
# Builds what the desktop bundle embeds: the web app and a release bantud,
# copied to where Tauri's externalBin expects it (binaries/bantud-<triple>).
#   scripts/prepare-desktop.sh [target-triple]
set -eu
cd "$(dirname "$0")/.."
target="${1:-$(rustc -vV | sed -n 's/^host: //p')}"
bun run build:web
cargo build --release --locked -p bantud --target "$target"
mkdir -p apps/desktop/src-tauri/binaries
cp "target/$target/release/bantud" "apps/desktop/src-tauri/binaries/bantud-$target"
echo "Prepared bantud for $target"
