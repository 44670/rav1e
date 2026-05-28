#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'USAGE'
usage: tools/o3yv-old3ds-sd-handoff.sh <command> [bundle_dir] [sd_root]

Copies an Old3DS run bundle to a physical SD-card root and imports the
hardware log back into the bundle for validation.

Commands:
  detect      List likely SD-card mount roots.
  install     Copy o3yvbench.3dsx to <sd_root>/3ds/o3yvbench/.
  import-log  Copy <sd_root>/o3yvbench.log to bundle old3ds-bench.log,
              then run tools/o3yv-old3ds-bundle-status.sh.

Defaults:
  bundle_dir  tmp/o3yv-old3ds-playable

Examples:
  tools/o3yv-old3ds-sd-handoff.sh detect
  tools/o3yv-old3ds-sd-handoff.sh install tmp/o3yv-old3ds-playable /media/$USER/OLD3DS
  tools/o3yv-old3ds-sd-handoff.sh import-log tmp/o3yv-old3ds-playable /media/$USER/OLD3DS

Use a physical Old3DS SD-card root for hardware proof, not an emulator sdmc
directory.
USAGE
}

command=${1:-}
bundle_dir=${2:-tmp/o3yv-old3ds-playable}
sd_root=${3:-}

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
