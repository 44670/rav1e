#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'USAGE'
usage: tools/o3yv-mp4-baseline.sh INPUT.yuv OUT_PREFIX [frames] [kbps]

Creates an H.264 Baseline-profile MP4 reference for the O3YV sample path:
  OUT_PREFIX.mp4
  OUT_PREFIX.dec.yuv
  OUT_PREFIX.psnr.log
  OUT_PREFIX.ffmpeg.log
  OUT_PREFIX_png/f_00000_mp4.png ...

Defaults: frames=100, kbps=8700
USAGE
}

if [[ "${1:-}" == "-h" || "${1:-}" == "--help" || $# -lt 2 ]]; then
  usage
  exit 0
fi

input=$1
out_prefix=$2
frames=${3:-100}
kbps=${4:-8700}

width=800
height=240
fps=24

mp4="${out_prefix}.mp4"
decoded="${out_prefix}.dec.yuv"
psnr_log="${out_prefix}.psnr.log"
ffmpeg_log="${out_prefix}.ffmpeg.log"
png_dir="${out_prefix}_png"

mkdir -p "$(dirname "$out_prefix")" "$png_dir"

ffmpeg -hide_banner -y \
  -f rawvideo -pix_fmt yuv420p -s:v "${width}x${height}" -r "$fps" \
  -i "$input" -frames:v "$frames" \
  -c:v libx264 -profile:v baseline -level:v 3.0 \
  -preset veryslow -tune psnr \
  -b:v "${kbps}k" -maxrate "${kbps}k" -bufsize "$((kbps * 2))k" \
  -pix_fmt yuv420p -movflags +faststart \
  "$mp4" 2> "$ffmpeg_log"

ffmpeg -hide_banner -y \
  -i "$mp4" -frames:v "$frames" \
  -f rawvideo -pix_fmt yuv420p \
  "$decoded" >> "$ffmpeg_log" 2>&1

ffmpeg -hide_banner \
  -f rawvideo -pix_fmt yuv420p -s:v "${width}x${height}" -r "$fps" \
  -i "$input" \
  -f rawvideo -pix_fmt yuv420p -s:v "${width}x${height}" -r "$fps" \
  -i "$decoded" \
  -lavfi "psnr=stats_file=${psnr_log}" \
  -frames:v "$frames" -f null - >> "$ffmpeg_log" 2>&1

ffmpeg -hide_banner -y \
  -f rawvideo -pix_fmt yuv420p -s:v "${width}x${height}" -r "$fps" \
  -i "$decoded" -frames:v "$frames" \
  -start_number 0 "${png_dir}/f_%05d_mp4.png" >> "$ffmpeg_log" 2>&1

ffprobe -v error -show_entries format=bit_rate,size,duration \
  -of default=nw=1 "$mp4"
