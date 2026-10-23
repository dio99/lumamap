#!/usr/bin/env bash
# Bygger allt som ska till en release i target/release-files/:
#   LuxCanvas-<ver>-x86_64.AppImage, .flatpak, luxcanvas_<ver>-1_amd64.deb, luxcanvas-<ver>-1.x86_64.rpm
#
# Kräver: cargo install cargo-deb cargo-generate-rpm, samt org.flatpak.Builder
# och runtime/SDK enligt README. Kör från var som helst.
set -euo pipefail
ROOT=$(cd "$(dirname "$0")/.." && pwd)
cd "$ROOT"
VER=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
OUT=target/release-files
mkdir -p "$OUT"
echo "== LuxCanvas $VER"

cargo build --release --locked

echo "== .deb"
cargo deb -p luxcanvas --no-build
cp "target/debian/luxcanvas_${VER}-1_amd64.deb" "$OUT/"

echo "== .rpm"
cargo generate-rpm -p crates/lc-app
cp "target/generate-rpm/luxcanvas-${VER}-1.x86_64.rpm" "$OUT/"

echo "== AppImage"
bash packaging/appimage/build.sh
cp target/appimage/LuxCanvas-x86_64.AppImage "$OUT/LuxCanvas-${VER}-x86_64.AppImage"

echo "== Flatpak"
flatpak run org.flatpak.Builder --user --force-clean --repo="$ROOT/target/flatpak-repo" \
    "$ROOT/target/flatpak" packaging/flatpak/io.github.dio99.LuxCanvas.yml
flatpak build-bundle --runtime-repo=https://dl.flathub.org/repo/flathub.flatpakrepo \
    target/flatpak-repo "$OUT/LuxCanvas-${VER}-x86_64.flatpak" io.github.dio99.LuxCanvas

echo "== Klart:"
ls -lh "$OUT"
