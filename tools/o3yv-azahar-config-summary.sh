#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'USAGE'
usage: tools/o3yv-azahar-config-summary.sh [qt-config.ini]

Prints one machine-readable Azahar configuration line for Old3DS evidence.

Defaults:
  qt-config.ini  $AZAHAR_CONFIG or ~/.config/azahar-emu/qt-config.ini

Required for status=pass:
  is_new_3ds=false
  cpu_clock_percentage=100
  simulate_3ds_gpu_timings=true
USAGE
}

if [[ "${1:-}" == "-h" || "${1:-}" == "--help" ]]; then
  usage
  exit 0
fi

config=${1:-${AZAHAR_CONFIG:-"$HOME/.config/azahar-emu/qt-config.ini"}}
display=${DISPLAY:-:0}

config_value() {
  local key=$1
  awk -F= -v key="$key" '
    $1 == key {
      print $2
      found = 1
      exit
    }
    END {
      if (!found) {
        exit 1
      }
    }
  ' "$config"
}

print_line() {
  local status=$1
  local reason=$2
  local is_new_3ds=${3:-unknown}
  local cpu_clock_percentage=${4:-unknown}
  local simulate_3ds_gpu_timings=${5:-unknown}
  local use_cpu_jit=${6:-unknown}
  printf 'azahar_config status=%s reason=%s config=%s display=%s is_new_3ds=%s cpu_clock_percentage=%s simulate_3ds_gpu_timings=%s use_cpu_jit=%s\n' \
    "$status" "$reason" "$config" "$display" "$is_new_3ds" \
    "$cpu_clock_percentage" "$simulate_3ds_gpu_timings" "$use_cpu_jit"
}

if [[ ! -f "$config" ]]; then
  print_line missing missing_config
  exit 1
fi

is_new_3ds=$(config_value is_new_3ds || printf 'unknown')
cpu_clock_percentage=$(config_value cpu_clock_percentage || printf 'unknown')
simulate_3ds_gpu_timings=$(
  config_value simulate_3ds_gpu_timings || printf 'unknown'
)
use_cpu_jit=$(config_value use_cpu_jit || printf 'unknown')

reasons=()
if [[ "$is_new_3ds" != "false" ]]; then
  reasons+=("is_new_3ds_${is_new_3ds}")
fi
if [[ "$cpu_clock_percentage" != "100" ]]; then
  reasons+=("cpu_clock_percentage_${cpu_clock_percentage}")
fi
if [[ "$simulate_3ds_gpu_timings" != "true" ]]; then
  reasons+=("simulate_3ds_gpu_timings_${simulate_3ds_gpu_timings}")
fi

if (( ${#reasons[@]} == 0 )); then
  print_line pass none "$is_new_3ds" "$cpu_clock_percentage" \
    "$simulate_3ds_gpu_timings" "$use_cpu_jit"
  exit 0
fi

reason=$(
  IFS=,
  printf '%s' "${reasons[*]}"
)
print_line fail "$reason" "$is_new_3ds" "$cpu_clock_percentage" \
  "$simulate_3ds_gpu_timings" "$use_cpu_jit"
exit 1
