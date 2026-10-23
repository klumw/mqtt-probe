#!/usr/bin/env bash
set -euo pipefail

PROJECT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
BINARY="${PROJECT_DIR}/target/release/mqtt-probe"
BIN_DIR="${HOME}/.local/bin"
APP_DIR="${HOME}/.local/share/applications"
ICON_DIR="${HOME}/.local/share/icons/hicolor/scalable/apps"

if [[ ! -x "${BINARY}" ]]; then
    echo "Release binary missing. Run 'cargo build --release' first." >&2
    exit 1
fi

install -Dm755 "${BINARY}" "${BIN_DIR}/mqtt-probe"
install -Dm644 "${PROJECT_DIR}/assets/mqtt-probe.svg" "${ICON_DIR}/mqtt-probe.svg"

mkdir -p "${APP_DIR}"
sed \
    -e "s|^Exec=.*|Exec=${BIN_DIR}/mqtt-probe|" \
    -e "s|^TryExec=.*|TryExec=${BIN_DIR}/mqtt-probe|" \
    "${PROJECT_DIR}/packaging/mqtt-probe.desktop" > "${APP_DIR}/mqtt-probe.desktop"
chmod 644 "${APP_DIR}/mqtt-probe.desktop"

# Remove launcher artifacts from the previous application name.
rm -f "${BIN_DIR}/mqtt-ui" "${APP_DIR}/mqtt-ui.desktop" "${ICON_DIR}/mqtt-ui.svg"

update-desktop-database "${APP_DIR}" >/dev/null 2>&1 || true
gtk-update-icon-cache -f -t "${HOME}/.local/share/icons/hicolor" >/dev/null 2>&1 || true

echo "MQTT Probe installed: ${APP_DIR}/mqtt-probe.desktop"
echo "The entry appears in the menu under Accessories."
