#!/bin/sh
set -eu

HOST_NAME="com.myapp.download_manager"

if [ "$(id -u)" -ne 0 ]; then
  echo "unregister-native-host-linux.sh must run as root for system-wide native messaging unregistration." >&2
  exit 1
fi

remove_manifest() {
  manifest_dir="$1"
  rm -f "$manifest_dir/$HOST_NAME.json"
}

remove_manifest "/etc/opt/chrome/native-messaging-hosts"
remove_manifest "/etc/chromium/native-messaging-hosts"
remove_manifest "/etc/opt/edge/native-messaging-hosts"
remove_manifest "/usr/lib/mozilla/native-messaging-hosts"
remove_manifest "/usr/lib64/mozilla/native-messaging-hosts"
