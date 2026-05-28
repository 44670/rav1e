#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'USAGE'
usage: tools/o3yv-old3ds-sd-handoff.sh <command> [bundle_dir] [sd_root|zip_path]

Copies an Old3DS run bundle to a physical SD-card root and imports the
hardware log back into the bundle for validation.

Commands:
  detect      List likely SD-card mount roots.
  install     Copy o3yvbench.3dsx to <sd_root>/3ds/o3yvbench/.
  export-zip  Write a zip whose contents can be extracted at an SD-card root.
  import-log  Copy <sd_root>/o3yvbench.log to bundle old3ds-bench.log,
              then run tools/o3yv-old3ds-bundle-status.sh.

Defaults:
  bundle_dir  tmp/o3yv-old3ds-playable

Examples:
  tools/o3yv-old3ds-sd-handoff.sh detect
  tools/o3yv-old3ds-sd-handoff.sh install tmp/o3yv-old3ds-playable /media/$USER/OLD3DS
  tools/o3yv-old3ds-sd-handoff.sh export-zip tmp/o3yv-old3ds-playable tmp/o3yv-old3ds-playable-sd.zip
  tools/o3yv-old3ds-sd-handoff.sh import-log tmp/o3yv-old3ds-playable /media/$USER/OLD3DS

Use a physical Old3DS SD-card root for hardware proof, not an emulator sdmc
directory.
USAGE
}

command=${1:-}
bundle_dir=${2:-tmp/o3yv-old3ds-playable}
sd_target=${3:-}
sd_root=$sd_target

if [[ "$command" == "-h" || "$command" == "--help" || -z "$command" ]]; then
  usage
  exit 0
fi

manifest="$bundle_dir/MANIFEST.env"

load_manifest() {
  if [[ ! -f "$manifest" ]]; then
    echo "missing bundle manifest: $manifest" >&2
    exit 1
  fi

  # MANIFEST.env is generated with printf %q specifically to be sourceable.
  # shellcheck disable=SC1090
  source "$manifest"

  for name in o3yvbench_3dsx hardware_log_copy expected_checksum; do
    if [[ -z "${!name:-}" ]]; then
      echo "missing $name in $manifest" >&2
      exit 1
    fi
  done
}

require_sd_root() {
  if [[ -z "$sd_root" ]]; then
    echo "missing sd_root" >&2
    echo "run: tools/o3yv-old3ds-sd-handoff.sh detect" >&2
    exit 1
  fi
  if [[ ! -d "$sd_root" ]]; then
    echo "missing SD root directory: $sd_root" >&2
    exit 1
  fi
}

detect_sd_roots() {
  shopt -s nullglob
  local roots=()
  roots+=(/media/"${USER:-}"/*)
  roots+=(/run/media/"${USER:-}"/*)
  roots+=(/Volumes/*)
  roots+=(/mnt/*)

  local found=0
  local root
  for root in "${roots[@]}"; do
    [[ -d "$root" ]] || continue
    case "$root" in
      /mnt/hgfs | /mnt/hgfs/*)
        continue
        ;;
    esac
    found=1
    local score=generic
    if [[ -d "$root/Nintendo 3DS" || -d "$root/3ds" ]]; then
      score=likely_3ds_sd
    fi
    printf 'sd_candidate status=%s root=%s\n' "$score" "$root"
  done

  if (( found == 0 )); then
    printf 'sd_candidate status=none\n'
  fi
}

case "$command" in
  detect)
    detect_sd_roots
    ;;
  install)
    load_manifest
    require_sd_root
    if [[ ! -f "$o3yvbench_3dsx" ]]; then
      echo "missing packaged .3dsx: $o3yvbench_3dsx" >&2
      exit 1
    fi
    app_dir="$sd_root/3ds/o3yvbench"
    mkdir -p "$app_dir"
    cp "$o3yvbench_3dsx" "$app_dir/o3yvbench.3dsx"
    if [[ -f "$bundle_dir/RUN_ON_OLD3DS.txt" ]]; then
      cp "$bundle_dir/RUN_ON_OLD3DS.txt" "$app_dir/RUN_ON_OLD3DS.txt"
    fi
    if [[ -f "$sd_root/o3yvbench.log" ]]; then
      printf 'warning=existing_sd_log_may_be_overwritten_or_stale path=%s\n' \
        "$sd_root/o3yvbench.log" >&2
    fi
    printf 'old3ds_sd_install status=pass app=%s source=%s expected_checksum=%s\n' \
      "$app_dir/o3yvbench.3dsx" "$o3yvbench_3dsx" "$expected_checksum"
    ;;
  export-zip)
    load_manifest
    zip_path=${sd_target:-"$bundle_dir-sd.zip"}
    if [[ ! -f "$o3yvbench_3dsx" ]]; then
      echo "missing packaged .3dsx: $o3yvbench_3dsx" >&2
      exit 1
    fi
    if ! command -v zip >/dev/null 2>&1; then
      echo "missing command: zip" >&2
      exit 1
    fi
    mkdir -p "$(dirname "$zip_path")"
    zip_abs=$(
      cd "$(dirname "$zip_path")"
      printf '%s/%s\n' "$(pwd)" "$(basename "$zip_path")"
    )
    tmp_root=$(mktemp -d "${TMPDIR:-/tmp}/o3yv-sd-zip-XXXXXX")
    cleanup() {
      rm -rf "$tmp_root"
    }
    trap cleanup EXIT
    app_dir="$tmp_root/3ds/o3yvbench"
    mkdir -p "$app_dir"
    cp "$o3yvbench_3dsx" "$app_dir/o3yvbench.3dsx"
    for item in RUN_ON_OLD3DS.txt MANIFEST.env expected.txt; do
      if [[ -f "$bundle_dir/$item" ]]; then
        cp "$bundle_dir/$item" "$app_dir/$item"
      fi
    done
    rm -f "$zip_abs"
    (
      cd "$tmp_root"
      zip -qr "$zip_abs" .
    )
    zip_bytes=$(wc -c <"$zip_abs")
    zip_sha256=$(sha256sum "$zip_abs" | awk '{ print $1; exit }')
    printf 'old3ds_sd_zip status=pass zip=%s bytes=%s sha256=%s app=3ds/o3yvbench/o3yvbench.3dsx expected_checksum=%s\n' \
      "$zip_abs" "$zip_bytes" "$zip_sha256" \
      "$expected_checksum"
    ;;
  import-log)
    load_manifest
    require_sd_root
    sd_log="$sd_root/o3yvbench.log"
    if [[ ! -f "$sd_log" ]]; then
      echo "missing hardware log: $sd_log" >&2
      exit 1
    fi
    cp "$sd_log" "$hardware_log_copy"
    printf 'old3ds_log_import status=pass source=%s dest=%s\n' \
      "$sd_log" "$hardware_log_copy"
    tools/o3yv-old3ds-bundle-status.sh "$bundle_dir"
    ;;
  *)
    echo "unknown command: $command" >&2
    usage >&2
    exit 1
    ;;
esac
