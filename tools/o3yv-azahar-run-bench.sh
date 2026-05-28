#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'USAGE'
usage: tools/o3yv-azahar-run-bench.sh <o3yvbench.3dsx> [out.log] [timeout_seconds] [wait_prefix]

Launches the Old3DS benchmark bundle in Azahar, waits for sdmc:/o3yvbench.log
to contain wait_prefix, copies that log to out.log, and stops Azahar.

Defaults:
  out.log          tmp/azahar-o3yvbench.log
  timeout_seconds 60
  wait_prefix     bench_result

Environment:
  AZAHAR           Azahar launcher path, default /opt/3ds/azahar
  AZAHAR_SDMC_DIR  Azahar sdmc directory, default ~/.local/share/azahar-emu/sdmc
  DISPLAY          X display, default :0
USAGE
}

if [[ "${1:-}" == "-h" || "${1:-}" == "--help" ]]; then
  usage
  exit 0
fi

rom=${1:?missing o3yvbench.3dsx}
out_log=${2:-tmp/azahar-o3yvbench.log}
timeout_seconds=${3:-60}
wait_prefix=${4:-bench_result}
azahar=${AZAHAR:-/opt/3ds/azahar}
display=${DISPLAY:-:0}
sdmc_dir=${AZAHAR_SDMC_DIR:-"$HOME/.local/share/azahar-emu/sdmc"}
sd_log="$sdmc_dir/o3yvbench.log"

if [[ ! -f "$rom" ]]; then
  echo "missing .3dsx: $rom" >&2
  exit 1
fi
if [[ ! -x "$azahar" ]]; then
  echo "missing executable Azahar launcher: $azahar" >&2
  exit 1
fi
if [[ ! "$timeout_seconds" =~ ^[0-9]+$ || "$timeout_seconds" == 0 ]]; then
  echo "timeout_seconds must be a positive integer" >&2
  exit 1
fi

mkdir -p "$sdmc_dir"
mkdir -p "$(dirname "$out_log")"

if [[ -f "$sd_log" ]]; then
  mv "$sd_log" "$sd_log.prev"
fi

DISPLAY="$display" "$azahar" -w "$rom" &
azahar_pid=$!

stop_azahar() {
  if kill -0 "$azahar_pid" >/dev/null 2>&1; then
    kill "$azahar_pid" >/dev/null 2>&1 || true
    sleep 1
  fi
  if kill -0 "$azahar_pid" >/dev/null 2>&1; then
    kill -9 "$azahar_pid" >/dev/null 2>&1 || true
  fi
}

trap stop_azahar EXIT

deadline=$((SECONDS + timeout_seconds))
while (( SECONDS < deadline )); do
  if [[ -f "$sd_log" ]] && grep -q "^${wait_prefix} " "$sd_log"; then
    cp "$sd_log" "$out_log"
    stop_azahar
    trap - EXIT
    echo "wrote Azahar bench log: $out_log"
    grep "^${wait_prefix} " "$out_log" | tail -1
    exit 0
  fi
  if ! kill -0 "$azahar_pid" >/dev/null 2>&1; then
    echo "Azahar exited before writing ${wait_prefix}" >&2
    if [[ -f "$sd_log" ]]; then
      cat "$sd_log" >&2
    fi
    exit 1
  fi
  sleep 0.25
done

echo "timed out waiting for ${wait_prefix} in $sd_log" >&2
if [[ -f "$sd_log" ]]; then
  cat "$sd_log" >&2
fi
exit 1
