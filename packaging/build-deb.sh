#!/usr/bin/env bash
# Build a Debian package (.deb) for MQTT Probe.
#
# Usage:
#   ./packaging/build-deb.sh
#
# The package is written to dist/mqtt-probe_<version>-<revision>_<arch>.deb.
# Requires: cargo, dpkg-deb, dpkg-shlibdeps, gzip.
set -euo pipefail
umask 022

PROJECT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$PROJECT_DIR"

PKG_NAME="mqtt-probe"
VERSION="$(sed -n 's/^version *= *"\([^"]*\)".*/\1/p' Cargo.toml | head -n 1)"
DEB_REVISION="${DEB_REVISION:-1}"
ARCH="$(dpkg --print-architecture)"
DEB_VERSION="${VERSION}-${DEB_REVISION}"
OUT_DIR="${PROJECT_DIR}/dist"
DEB_PATH="${OUT_DIR}/${PKG_NAME}_${DEB_VERSION}_${ARCH}.deb"
TARGET_DIR="${CARGO_TARGET_DIR:-${PROJECT_DIR}/target}"
[[ "$TARGET_DIR" = /* ]] || TARGET_DIR="${PROJECT_DIR}/${TARGET_DIR}"
export CARGO_TARGET_DIR="$TARGET_DIR"

MAINTAINER_NAME="${MAINTAINER_NAME:-$(git config user.name 2>/dev/null || true)}"
MAINTAINER_EMAIL="${MAINTAINER_EMAIL:-$(git config user.email 2>/dev/null || true)}"

if [[ -z "$VERSION" ]]; then
    echo "error: could not read the package version from Cargo.toml" >&2
    exit 1
fi
if [[ -z "$MAINTAINER_NAME" || -z "$MAINTAINER_EMAIL" ]]; then
    echo "error: set MAINTAINER_NAME and MAINTAINER_EMAIL (or configure git user.name/user.email)" >&2
    exit 1
fi
MAINTAINER="${MAINTAINER_NAME} <${MAINTAINER_EMAIL}>"

STAGE="$(mktemp -d)"
SHLIB_DIR="$(mktemp -d)"
trap 'rm -rf "$STAGE" "$SHLIB_DIR"' EXIT

echo "==> Building ${PKG_NAME} ${VERSION} (release)"
cargo build --release --locked

if [[ ! -x "${TARGET_DIR}/release/${PKG_NAME}" ]]; then
    echo "error: ${TARGET_DIR}/release/${PKG_NAME} was not produced" >&2
    exit 1
fi

echo "==> Staging files"
install -Dm755 "${TARGET_DIR}/release/${PKG_NAME}" "$STAGE/usr/bin/${PKG_NAME}"
install -Dm644 "packaging/${PKG_NAME}.desktop" \
    "$STAGE/usr/share/applications/${PKG_NAME}.desktop"
install -Dm644 "assets/${PKG_NAME}.svg" \
    "$STAGE/usr/share/icons/hicolor/scalable/apps/${PKG_NAME}.svg"
for size in 16 24 32 48 64 128 256 512; do
    install -Dm644 "packaging/icons/hicolor/${size}x${size}/apps/${PKG_NAME}.png" \
        "$STAGE/usr/share/icons/hicolor/${size}x${size}/apps/${PKG_NAME}.png"
done
install -Dm644 "packaging/icons/hicolor/256x256/apps/${PKG_NAME}.png" \
    "$STAGE/usr/share/pixmaps/${PKG_NAME}.png"
install -Dm644 "packaging/${PKG_NAME}.metainfo.xml" \
    "$STAGE/usr/share/metainfo/${PKG_NAME}.metainfo.xml"
install -Dm644 "packaging/deb/copyright" \
    "$STAGE/usr/share/doc/${PKG_NAME}/copyright"
install -Dm644 LICENSE "$STAGE/usr/share/doc/${PKG_NAME}/LICENSE"
install -Dm644 NOTICE "$STAGE/usr/share/doc/${PKG_NAME}/NOTICE"

# Debian changelog (policy 4.7)
{
    printf '%s (%s) unstable; urgency=medium\n\n' "$PKG_NAME" "$DEB_VERSION"
    printf '  * Upstream release %s.\n\n' "$VERSION"
    printf -- ' -- %s  %s\n' "$MAINTAINER" "$(date -R)"
} | gzip -9n > "$STAGE/usr/share/doc/${PKG_NAME}/changelog.Debian.gz"
chmod 644 "$STAGE/usr/share/doc/${PKG_NAME}/changelog.Debian.gz"

# Compute shared-library dependencies with dpkg-shlibdeps. It expects a
# debian/control next to the working directory, so provide a throwaway one.
mkdir -p "$SHLIB_DIR/debian"
cat > "$SHLIB_DIR/debian/control" <<EOF
Source: ${PKG_NAME}
Section: net
Priority: optional
Maintainer: ${MAINTAINER}
Standards-Version: 4.7.0
Rules-Requires-Root: no

Package: ${PKG_NAME}
Architecture: any
Depends: \${shlibs:Depends}, \${misc:Depends}
Description: temporary control file for dpkg-shlibdeps
 Used only while building the real package.
EOF

SHLIBS="$(cd "$SHLIB_DIR" && dpkg-shlibdeps -O "$STAGE/usr/bin/${PKG_NAME}" 2>/dev/null \
    | sed -n 's/^shlibs:Depends=//p')"
if [[ -z "$SHLIBS" ]]; then
    echo "warning: dpkg-shlibdeps produced no dependencies; falling back to libc6, libgcc-s1" >&2
    SHLIBS="libc6, libgcc-s1"
fi

INSTALLED_SIZE="$(du -sk "$STAGE/usr" | cut -f1)"

echo "==> Writing control metadata"
mkdir -p "$STAGE/DEBIAN"
cat > "$STAGE/DEBIAN/control" <<EOF
Package: ${PKG_NAME}
Version: ${DEB_VERSION}
Section: net
Priority: optional
Architecture: ${ARCH}
Maintainer: ${MAINTAINER}
Installed-Size: ${INSTALLED_SIZE}
Depends: ${SHLIBS}
Recommends: libgl1, libx11-6, libxkbcommon0, libwayland-client0
Homepage: https://github.com/klumw/mqtt-probe
Description: MQTT broker explorer for topics, payloads and live diagnostics
 MQTT Probe is a native desktop application for exploring MQTT brokers.
 It shows topics in a live tree, inspects payloads as JSON, text, diff
 or chart, and can publish messages with configurable QoS and retain
 flags. It supports plain TCP, TLS, mutual TLS, WebSockets and secure
 WebSockets, and is available in six languages.
EOF
chmod 755 "$STAGE/DEBIAN"
chmod 644 "$STAGE/DEBIAN/control"

if command -v desktop-file-validate >/dev/null 2>&1; then
    desktop-file-validate "packaging/${PKG_NAME}.desktop"
fi

echo "==> Creating ${DEB_PATH}"
chmod 755 "$STAGE"
mkdir -p "$OUT_DIR"
dpkg-deb --root-owner-group --build "$STAGE" "$DEB_PATH"

echo
echo "Created: ${DEB_PATH}"
echo "Install with: sudo apt install \"${DEB_PATH}\""
