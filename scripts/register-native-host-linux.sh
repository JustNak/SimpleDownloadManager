#!/bin/sh
set -eu

HOST_NAME="com.myapp.download_manager"
HOST_BINARY_PATH="${1:-/usr/bin/simple-download-manager-native-host}"
CHROMIUM_EXTENSION_ID="${SDM_CHROMIUM_EXTENSION_ID:-pkaojpfpjieklhinoibjibmjldohlmbb}"
FIREFOX_EXTENSION_ID="${SDM_FIREFOX_EXTENSION_ID:-simple-download-manager@example.com}"

if [ "$(id -u)" -ne 0 ]; then
  echo "register-native-host-linux.sh must run as root for system-wide native messaging registration." >&2
  exit 1
fi

if [ ! -x "$HOST_BINARY_PATH" ]; then
  echo "Native host binary is missing or not executable: $HOST_BINARY_PATH" >&2
  exit 1
fi

install_chromium_manifest() {
  manifest_dir="$1"
  mkdir -p "$manifest_dir"
  cat > "$manifest_dir/$HOST_NAME.json" <<EOF
{
  "name": "$HOST_NAME",
  "description": "Simple Download Manager native messaging host",
  "path": "$HOST_BINARY_PATH",
  "type": "stdio",
  "allowed_origins": [
    "chrome-extension://$CHROMIUM_EXTENSION_ID/"
  ]
}
EOF
  chmod 0644 "$manifest_dir/$HOST_NAME.json"
}

install_firefox_manifest() {
  manifest_dir="$1"
  mkdir -p "$manifest_dir"
  cat > "$manifest_dir/$HOST_NAME.json" <<EOF
{
  "name": "$HOST_NAME",
  "description": "Simple Download Manager native messaging host",
  "path": "$HOST_BINARY_PATH",
  "type": "stdio",
  "allowed_extensions": [
    "$FIREFOX_EXTENSION_ID"
  ]
}
EOF
  chmod 0644 "$manifest_dir/$HOST_NAME.json"
}

install_chromium_manifest "/etc/opt/chrome/native-messaging-hosts"
install_chromium_manifest "/etc/chromium/native-messaging-hosts"
install_chromium_manifest "/etc/opt/edge/native-messaging-hosts"
install_firefox_manifest "/usr/lib/mozilla/native-messaging-hosts"

if [ -d /usr/lib64/mozilla ] || [ ! -d /usr/lib/mozilla ]; then
  install_firefox_manifest "/usr/lib64/mozilla/native-messaging-hosts"
fi
