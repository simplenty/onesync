#!/usr/bin/env bash
#
# OneSync unified packaging script.
# Builds a release binary and produces .deb, .rpm, and .AppImage.
# All outputs go under build/.
#
# Usage:
#   ./resources/package.sh [all|deb|rpm|appimage]
#
set -euo pipefail

PROJECT_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
APP_NAME="onesync"
APP_ID="io.github.simplenty.onesync"
BINARY="OneSync"
VERSION="$(cargo metadata --manifest-path "$PROJECT_ROOT/Cargo.toml" --no-deps --format-version 1 | jq -r '.packages[0].version')"

# Map uname arch to consistent naming: amd64 / arm64
RELEASE_ARCH="$(uname -m)"
case "$RELEASE_ARCH" in
    x86_64)  ARCH="amd64" ;;
    aarch64) ARCH="arm64" ;;
    armv7l)  ARCH="armhf" ;;
    *)       ARCH="$RELEASE_ARCH" ;;
esac

BUILD="$PROJECT_ROOT/build"
export APPIMAGE_EXTRACT_AND_RUN=1

# ---------------------------------------------------------------------------
log() { echo ">> $*"; }
ok()  { echo "OK  $*"; }

ensure_cargo_tool() {
    local crate="$1"
    # For cargo-deb and cargo-generate-rpm, the binary is cargo-<name>
    # so we check for 'cargo deb --version' but install 'cargo-deb'
    if [[ "$crate" == "deb" ]]; then
        if ! cargo deb --version &>/dev/null; then
            log "Installing cargo-deb"
            cargo install cargo-deb
        fi
    elif [[ "$crate" == "generate-rpm" ]]; then
        if ! cargo generate-rpm --version &>/dev/null; then
            log "Installing cargo-generate-rpm"
            cargo install cargo-generate-rpm
        fi
    else
        if ! cargo "$crate" --version &>/dev/null; then
            log "Installing cargo-$crate"
            cargo install "$crate"
        fi
    fi
}

download_tool() {
    local url="$1" dest="$2"
    if [[ -x "$dest" ]]; then return 0; fi
    log "Downloading $(basename "$dest")"
    mkdir -p "$(dirname "$dest")"
    curl -fsSL "$url" -o "$dest"
    chmod +x "$dest"
}

# ---------------------------------------------------------------------------
build_release() {
    log "Building release binary"
    cargo build --release --manifest-path "$PROJECT_ROOT/Cargo.toml"
    ok "target/release/$BINARY"
}

build_deb() {
    ensure_cargo_tool deb
    log "Packaging .deb -> $BUILD/"
    cargo deb --manifest-path "$PROJECT_ROOT/Cargo.toml" --no-build --output "$BUILD"

    # Rename to onesync-{version}-{arch}.deb
    local deb
    deb=$(find "$BUILD" -maxdepth 1 -name 'onesync_*.deb' -printf '%T@ %p\n' 2>/dev/null | sort -rn | head -1 | cut -d' ' -f2-)
    if [[ -f "$deb" ]]; then
        local new="$BUILD/onesync-${VERSION}-${ARCH}.deb"
        mv -v "$deb" "$new"
        ok "$(basename "$new")"
    fi
}

build_rpm() {
    ensure_cargo_tool generate-rpm
    log "Packaging .rpm -> $BUILD/"
    (cd "$PROJECT_ROOT" && cargo generate-rpm --arch "$RELEASE_ARCH")

    # Rename to onesync-{version}-{arch}.rpm
    local rpm
    rpm=$(find "$PROJECT_ROOT/target/generate-rpm" -name '*.rpm' -printf '%T@ %p\n' 2>/dev/null | sort -rn | head -1 | cut -d' ' -f2-)
    if [[ -f "$rpm" ]]; then
        local new="$BUILD/onesync-${VERSION}-${ARCH}.rpm"
        cp -v "$rpm" "$new"
        ok "$(basename "$new")"
    fi
}

build_appimage() {
    local ld="$BUILD/tools/linuxdeploy"
    local ait="$BUILD/tools/appimagetool"

    download_tool "https://github.com/linuxdeploy/linuxdeploy/releases/download/continuous/linuxdeploy-${RELEASE_ARCH}.AppImage" "$ld"
    download_tool "https://github.com/AppImage/appimagetool/releases/download/continuous/appimagetool-${RELEASE_ARCH}.AppImage" "$ait"

    export ARCH
    export OUTPUT="$BUILD/onesync-${VERSION}-${ARCH}.AppImage"

    local appdir="$BUILD/AppDir"
    log "Preparing AppDir"
    rm -rf "$appdir"
    mkdir -p "$appdir/usr/bin" "$appdir/usr/share/applications" "$appdir/usr/share/icons/hicolor"

    cp "$PROJECT_ROOT/target/release/$BINARY" "$appdir/usr/bin/$APP_NAME"
    cp "$PROJECT_ROOT/resources/OneSync.desktop" "$appdir/$APP_ID.desktop"

    for size in 16 24 32 48 64 128 256 512; do
        local src="$PROJECT_ROOT/resources/icons/${size}.png"
        [[ -f "$src" ]] || continue
        mkdir -p "$appdir/usr/share/icons/hicolor/${size}x${size}/apps"
        cp "$src" "$appdir/usr/share/icons/hicolor/${size}x${size}/apps/$APP_ID.png"
    done

    log "Running linuxdeploy"
    "$ld" --appdir "$appdir" \
        --desktop-file "$appdir/$APP_ID.desktop" \
        --icon-file "$appdir/usr/share/icons/hicolor/256x256/apps/$APP_ID.png" \
        --deploy-deps-only "$appdir/usr/bin/$APP_NAME"

    log "Bundling GSettings schemas"
    local ss="/usr/share/glib-2.0/schemas"
    local sd="$appdir/usr/share/glib-2.0/schemas"
    mkdir -p "$sd"
    for xml in "$ss"/org.gtk.gtk4.Settings.FileChooser.gschema.xml \
               "$ss"/org.gtk.gtk4.Settings.ColorChooser.gschema.xml \
               "$ss"/org.gtk.gtk4.Settings.EmojiChooser.gschema.xml \
               "$ss"/org.gtk.gtk4.Settings.Debug.gschema.xml \
               "$ss"/org.gtk.gtk4.Inspector.gschema.xml \
               "$ss"/org.gtk.Settings.FileChooser.gschema.xml \
               "$ss"/org.gtk.Settings.ColorChooser.gschema.xml \
               "$ss"/org.gtk.Settings.EmojiChooser.gschema.xml \
               "$ss"/org.gtk.Settings.Debug.gschema.xml \
               "$ss"/org.gnome.desktop.interface.gschema.xml \
               "$ss"/org.gnome.desktop.enums.xml \
               "$ss"/org.adw.gschema.xml; do
        [[ -f "$xml" ]] && cp "$xml" "$sd/"
    done
    glib-compile-schemas "$sd" 2>/dev/null || true

    log "Writing AppRun"
    rm -f "$appdir/AppRun"
    cat > "$appdir/AppRun" << 'APPRUN_EOF'
#!/usr/bin/env bash
set -e
APPDIR="${APPDIR:-$(cd "$(dirname "$0")" && pwd)}"
export GSETTINGS_SCHEMA_DIR="${APPDIR}/usr/share/glib-2.0/schemas"
exec "${APPDIR}/usr/bin/onesync" "$@"
APPRUN_EOF
    chmod +x "$appdir/AppRun"


    log "Creating AppImage"
    "$ait" --no-appstream "$appdir" "$OUTPUT"
    ok "$(basename "$OUTPUT")"
}

# ---------------------------------------------------------------------------
mkdir -p "$BUILD"
TARGET="${1:-all}"

case "$TARGET" in
    all)      build_release; build_deb; build_rpm; build_appimage ;;
    deb)      build_release; build_deb ;;
    rpm)      build_release; build_rpm ;;
    appimage) build_release; build_appimage ;;
    *)        echo "Usage: $0 [all|deb|rpm|appimage]" >&2; exit 1 ;;
esac

echo ""
ok "Artifacts in $BUILD/"
ls -lh "$BUILD/"
