#!/usr/bin/env bash
# Bygger LumaMap som AppImage: target/appimage/LumaMap-x86_64.AppImage
#
# Hämtar linuxdeploy och dess GStreamer-tillägg första gången (till
# packaging/appimage/tools/). GStreamer-pluginerna från systemet packas med,
# så att video fungerar även där de saknas. Grafikdrivrutinerna (Vulkan/GL)
# tas från datorn som kör AppImagen.
set -euo pipefail
cd "$(dirname "$0")/../.."
ROOT=$PWD
TOOLS=$ROOT/packaging/appimage/tools
OUT=$ROOT/target/appimage
APPDIR=$OUT/AppDir
ID=io.github.dio99.LumaMap

fetch() {
    if [ ! -x "$TOOLS/$1" ]; then
        echo "Hämtar $1"
        curl -fsSL -o "$TOOLS/$1" "$2"
        chmod +x "$TOOLS/$1"
    fi
}
mkdir -p "$TOOLS"
fetch linuxdeploy-x86_64.AppImage \
    https://github.com/linuxdeploy/linuxdeploy/releases/download/continuous/linuxdeploy-x86_64.AppImage
fetch linuxdeploy-plugin-gstreamer.sh \
    https://raw.githubusercontent.com/linuxdeploy/linuxdeploy-plugin-gstreamer/master/linuxdeploy-plugin-gstreamer.sh
# Packa upp linuxdeploy en gång: då behövs ingen FUSE, och GStreamer-tillägget
# hittar den patchelf som följer med.
if [ ! -x "$TOOLS/linuxdeploy/AppRun" ]; then
    (cd "$TOOLS" && ./linuxdeploy-x86_64.AppImage --appimage-extract > /dev/null && rm -rf linuxdeploy && mv squashfs-root linuxdeploy)
fi

cargo build --release

rm -rf "$APPDIR"
install -Dm755 target/release/lumamap "$APPDIR/usr/bin/lumamap"
install -Dm644 packaging/linux/$ID.metainfo.xml "$APPDIR/usr/share/metainfo/$ID.metainfo.xml"
install -Dm644 packaging/linux/$ID.mime.xml "$APPDIR/usr/share/mime/packages/$ID.xml"
# Exemplen följer med så att man kan prova direkt.
mkdir -p "$APPDIR/usr/share/lumamap"
cp -r examples "$APPDIR/usr/share/lumamap/"

# Tilläggen och patchelf letas upp via PATH.
export PATH="$TOOLS:$TOOLS/linuxdeploy/usr/bin:$PATH"
export APPIMAGE_EXTRACT_AND_RUN=1
# Hårdvaruavkodare (nvcodec, va) och flera strömprotokoll finns i "bad".
export GSTREAMER_INCLUDE_BAD_PLUGINS=1
export OUTPUT="$OUT/LumaMap-x86_64.AppImage"
cd "$OUT"
"$TOOLS/linuxdeploy/AppRun" \
    --appdir "$APPDIR" \
    --desktop-file "$ROOT/packaging/linux/$ID.desktop" \
    --icon-file "$ROOT/packaging/linux/$ID.svg" \
    --plugin gstreamer \
    --output appimage
echo "Klar: $OUTPUT"
