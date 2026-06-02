#!/usr/bin/env bash
set -euo pipefail

targets="x64"
while [ "$#" -gt 0 ]; do
  case "$1" in
    --targets)
      targets="$2"
      shift 2
      ;;
    *)
      echo "Unsupported argument: $1" >&2
      exit 1
      ;;
  esac
done

workspace_root="$(cd "$(dirname "$0")/.." && pwd)"
release_root="$workspace_root/release"
desktop_tauri_root="$workspace_root/apps/desktop/src-tauri"
host_root="$workspace_root/apps/native-host"
release_temp_root="$workspace_root/.tmp/release-linux"

if [ -z "${TAURI_SIGNING_PRIVATE_KEY:-}" ]; then
  default_key="$HOME/.simple-download-manager/tauri-updater.key"
  if [ -n "${SDM_TAURI_SIGNING_PRIVATE_KEY_PATH:-}" ] && [ -f "$SDM_TAURI_SIGNING_PRIVATE_KEY_PATH" ]; then
    export TAURI_SIGNING_PRIVATE_KEY="$(cat "$SDM_TAURI_SIGNING_PRIVATE_KEY_PATH")"
  elif [ -f "$default_key" ]; then
    export TAURI_SIGNING_PRIVATE_KEY="$(cat "$default_key")"
  else
    echo "TAURI_SIGNING_PRIVATE_KEY is required to build signed updater artifacts." >&2
    exit 1
  fi
fi

if [ -z "${TAURI_SIGNING_PRIVATE_KEY_PASSWORD+x}" ]; then
  if [ -n "${SDM_TAURI_SIGNING_PRIVATE_KEY_PASSWORD_PATH:-}" ] && [ -f "$SDM_TAURI_SIGNING_PRIVATE_KEY_PASSWORD_PATH" ]; then
    export TAURI_SIGNING_PRIVATE_KEY_PASSWORD="$(cat "$SDM_TAURI_SIGNING_PRIVATE_KEY_PASSWORD_PATH")"
  else
    export TAURI_SIGNING_PRIVATE_KEY_PASSWORD=""
  fi
fi

rm -rf "$release_root" "$release_temp_root"
mkdir -p "$release_root/bundle/appimage" "$release_root/bundle/deb" "$release_root/bundle/rpm" "$release_temp_root"

cd "$workspace_root"
npm run build:extension
npm run build:desktop

IFS=',' read -ra target_names <<< "$targets"
metadata_targets=()
for target_name in "${target_names[@]}"; do
  target_name="$(echo "$target_name" | xargs)"
  case "$target_name" in
    x64|amd64|x86_64|x86_64-unknown-linux-gnu)
      rust_target="x86_64-unknown-linux-gnu"
      metadata_targets+=("linux-x64")
      ;;
    *)
      echo "Unsupported Linux release target: $target_name" >&2
      exit 1
      ;;
  esac

  cargo build --release --manifest-path "$host_root/Cargo.toml" --target "$rust_target"

  target_config_path="$release_temp_root/tauri-$target_name.conf.json"
  node ./scripts/prepare-release.mjs --target "$rust_target" --config-out "$target_config_path"

  bundle_dir="$desktop_tauri_root/target/$rust_target/release/bundle"
  rm -rf "$bundle_dir"

  npm run tauri:build --workspace @myapp/desktop -- \
    --target "$rust_target" \
    --bundles deb,rpm,appimage \
    --config "$target_config_path" \
    -- \
    --bin simple-download-manager-desktop-backend

  cp "$bundle_dir/appimage/"* "$release_root/bundle/appimage/"
  cp "$bundle_dir/deb/"* "$release_root/bundle/deb/"
  cp "$bundle_dir/rpm/"* "$release_root/bundle/rpm/"
done

cp "$workspace_root/config/release.json" "$release_root/release.json"
metadata_targets_csv="$(IFS=,; echo "${metadata_targets[*]}")"
node ./scripts/updater-release.mjs --targets "$metadata_targets_csv"

echo "Linux release artifacts written to $release_root"
