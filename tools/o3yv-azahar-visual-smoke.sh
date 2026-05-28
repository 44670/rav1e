#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'USAGE'
usage: tools/o3yv-azahar-visual-smoke.sh <o3yvbench.3dsx> [out_dir] [timeout_seconds]

Launches o3yvbench.3dsx in Azahar, waits for playback_result, captures two
emulator-window screenshots, and checks that the video content is nonblank and
changing. This is only a host/emulator visual smoke test; it is not hardware
timing proof.

Defaults:
  out_dir          tmp/azahar-visual-smoke
  timeout_seconds 90

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
out_dir=${2:-tmp/azahar-visual-smoke}
timeout_seconds=${3:-90}
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

for command in xdotool xwd convert magick compare; do
  if ! command -v "$command" >/dev/null 2>&1; then
    echo "missing command: $command" >&2
    exit 1
  fi
done

mkdir -p "$sdmc_dir" "$out_dir"
report="$out_dir/visual-smoke.txt"

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
  if [[ -f "$sd_log" ]] && grep -q '^playback_result ' "$sd_log"; then
    break
  fi
  if ! kill -0 "$azahar_pid" >/dev/null 2>&1; then
    echo "Azahar exited before writing playback_result" >&2
    if [[ -f "$sd_log" ]]; then
      cat "$sd_log" >&2
    fi
    exit 1
  fi
  sleep 0.25
done

if [[ ! -f "$sd_log" ]] || ! grep -q '^playback_result ' "$sd_log"; then
  echo "timed out waiting for playback_result in $sd_log" >&2
  exit 1
fi
cp "$sd_log" "$out_dir/o3yvbench.log"

window_id=
for _ in $(seq 1 20); do
  window_id=$(
    DISPLAY="$display" xdotool search --name 'Azahar 2126' 2>/dev/null \
      | tail -1 || true
  )
  if [[ -n "$window_id" ]]; then
    break
  fi
  sleep 0.25
done
if [[ -z "$window_id" ]]; then
  echo "failed to find Azahar window" >&2
  exit 1
fi

sleep 0.5
DISPLAY="$display" xwd -silent -id "$window_id" -out "$out_dir/window-1.xwd"
sleep 1
DISPLAY="$display" xwd -silent -id "$window_id" -out "$out_dir/window-2.xwd"

convert "$out_dir/window-1.xwd" "$out_dir/window-1.png"
convert "$out_dir/window-2.xwd" "$out_dir/window-2.png"

magick "$out_dir/window-1.png" -crop 960x398+0+22 "$out_dir/content-1.png"
magick "$out_dir/window-2.png" -crop 960x398+0+22 "$out_dir/content-2.png"

frame1_colors=$(magick "$out_dir/content-1.png" -format '%k' info:)
frame2_colors=$(magick "$out_dir/content-2.png" -format '%k' info:)
frame1_stddev=$(magick "$out_dir/content-1.png" -format '%[standard-deviation]' info:)
frame2_stddev=$(magick "$out_dir/content-2.png" -format '%[standard-deviation]' info:)
frame1_entropy=$(magick "$out_dir/content-1.png" -format '%[entropy]' info:)
frame2_entropy=$(magick "$out_dir/content-2.png" -format '%[entropy]' info:)
rmse=$(
  compare -metric RMSE "$out_dir/content-1.png" "$out_dir/content-2.png" null: \
    2>&1 >/dev/null || true
)
ae=$(
  compare -metric AE "$out_dir/content-1.png" "$out_dir/content-2.png" null: \
    2>&1 >/dev/null || true
)

rmse_normalized=$(
  sed -n 's/.*(\([0-9.][0-9.]*\)).*/\1/p' <<<"$rmse" | head -1
)
rmse_normalized=${rmse_normalized:-0}

status=pass
if (( frame1_colors < 32 || frame2_colors < 32 )); then
  status=fail
fi
if ! awk -v a="$ae" 'BEGIN { exit !(a >= 1000) }'; then
  status=fail
fi
if ! awk -v r="$rmse_normalized" 'BEGIN { exit !(r >= 0.001) }'; then
  status=fail
fi

{
  printf 'azahar_visual_smoke status=%s window_id=%s\n' "$status" "$window_id"
  printf 'frame1 colors=%s stddev=%s entropy=%s png=%s\n' \
    "$frame1_colors" "$frame1_stddev" "$frame1_entropy" \
    "$out_dir/content-1.png"
  printf 'frame2 colors=%s stddev=%s entropy=%s png=%s\n' \
    "$frame2_colors" "$frame2_stddev" "$frame2_entropy" \
    "$out_dir/content-2.png"
  printf 'diff ae=%s rmse=%s rmse_normalized=%s\n' \
    "$ae" "$rmse" "$rmse_normalized"
  grep '^playback_result ' "$out_dir/o3yvbench.log" | tail -1
} >"$report"

cat "$report"
stop_azahar
trap - EXIT

if [[ "$status" != "pass" ]]; then
  exit 1
fi
