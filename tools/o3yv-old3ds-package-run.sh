#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'USAGE'
usage: tools/o3yv-old3ds-package-run.sh [input.o3yv] [out_dir] [iterations] [target_us]

Builds the Old3DS timing harness and writes a small run bundle containing:
  - o3yvbench.3dsx
  - expected host checksum metadata
  - MANIFEST.env with exact stream/build/check parameters
  - the exact log-verification command to run after copying sdmc:/o3yvbench.log

Defaults:
  input.o3yv  tmp/reencode_lazy128_current.o3yv
  out_dir     tmp/o3yv-old3ds-run
  iterations  8
  target_us   15000
USAGE
}

if [[ "${1:-}" == "-h" || "${1:-}" == "--help" ]]; then
  usage
  exit 0
fi

input=${1:-tmp/reencode_lazy128_current.o3yv}
out_dir=${2:-tmp/o3yv-old3ds-run}
iterations=${3:-8}
target_us=${4:-15000}

if [[ ! -f "$input" ]]; then
  echo "missing input stream: $input" >&2
  exit 1
fi
if [[ ! "$iterations" =~ ^[0-9]+$ || "$iterations" == 0 ]]; then
  echo "iterations must be a positive integer" >&2
  exit 1
fi
if [[ ! "$target_us" =~ ^[0-9]+$ || "$target_us" == 0 ]]; then
  echo "target_us must be a positive integer" >&2
  exit 1
fi

tools/o3yv-old3ds-build-harness.sh "$input" "$iterations" "$target_us"

if [[ ! -f old3ds/build/o3yvbench.3dsx ]]; then
  echo "build did not produce old3ds/build/o3yvbench.3dsx" >&2
  exit 1
fi

mkdir -p "$out_dir"
cp old3ds/build/o3yvbench.3dsx "$out_dir/o3yvbench.3dsx"

expected=$(
  tools/o3yv-old3ds-expected-checksum.sh "$input" "$iterations"
)
frames_per_iteration=$(
  awk -F= '/^frames_per_iteration=/ { print $2; exit }' <<<"$expected"
)
if [[ -z "$frames_per_iteration" ]]; then
  echo "failed to parse expected frames_per_iteration" >&2
  exit 1
fi
checksum=$(
  awk -F= '/^checksum=/ { print $2; exit }' <<<"$expected"
)
if [[ -z "$checksum" ]]; then
  echo "failed to parse expected checksum" >&2
  exit 1
fi
printf '%s\n' "$expected" >"$out_dir/expected.txt"
sha256sum "$out_dir/o3yvbench.3dsx" >"$out_dir/o3yvbench.3dsx.sha256"
bench_sha256=$(
  awk '{ print $1; exit }' "$out_dir/o3yvbench.3dsx.sha256"
)
input_bytes=$(wc -c <"$input")
bench_bytes=$(wc -c <"$out_dir/o3yvbench.3dsx")
repo_commit=$(
  git rev-parse --short HEAD 2>/dev/null || printf 'unknown'
)
generated_at_utc=$(date -u '+%Y-%m-%dT%H:%M:%SZ')

printf -v verify_bench_command \
  'tools/o3yv-old3ds-verify-log.sh %q %q %q %q' \
  "$out_dir/old3ds-bench.log" "$input" "$iterations" "$target_us"
printf -v verify_playback_command \
  'tools/o3yv-old3ds-check-playback-log.sh %q %q %q %q %q %q' \
  "$out_dir/old3ds-bench.log" "41666" "$frames_per_iteration" "24" \
  "y2r" "direct_planes"
printf -v report_command \
  'tools/o3yv-old3ds-playability-report.sh %q %q %q %q %q %q' \
  "$out_dir/old3ds-bench.log" "$input" "$iterations" "$target_us" \
  "41666" "real_old3ds"
printf -v azahar_timing_command \
  'tools/o3yv-azahar-run-bench.sh %q %q %q %q' \
  "$out_dir/o3yvbench.3dsx" "$out_dir/azahar-playback.log" "120" \
  "playback_result"
printf -v azahar_visual_command \
  'tools/o3yv-azahar-visual-smoke.sh %q %q %q' \
  "$out_dir/o3yvbench.3dsx" "$out_dir/azahar-visual-smoke" "120"
printf -v azahar_config_command \
  'tools/o3yv-azahar-config-summary.sh'
printf -v azahar_repeat_command \
  'tools/o3yv-azahar-repeat-bench.sh %q %q %q %q %q %q %q %q' \
  "$out_dir/o3yvbench.3dsx" "$out_dir/azahar-repeat-bench" "3" \
  "120" "$input" "$iterations" "$target_us" "41666"
printf -v bundle_status_command \
  'tools/o3yv-old3ds-bundle-status.sh %q' \
  "$out_dir"
printf -v sd_detect_command \
  'tools/o3yv-old3ds-sd-handoff.sh detect'
printf -v sd_install_command \
  'tools/o3yv-old3ds-sd-handoff.sh install %q /path/to/3ds-sd-root' \
  "$out_dir"
printf -v sd_import_log_command \
  'tools/o3yv-old3ds-sd-handoff.sh import-log %q /path/to/3ds-sd-root' \
  "$out_dir"

manifest_kv() {
  printf '%s=%q\n' "$1" "$2"
}

{
  manifest_kv o3yv_bundle_format 1
  manifest_kv generated_at_utc "$generated_at_utc"
  manifest_kv repo_commit "$repo_commit"
  manifest_kv input_stream "$input"
  manifest_kv input_bytes "$input_bytes"
  manifest_kv iterations "$iterations"
  manifest_kv bench_target_us "$target_us"
  manifest_kv playback_target_us 41666
  manifest_kv playback_fps 24
  manifest_kv playback_renderer y2r
  manifest_kv playback_output_mode direct_planes
  manifest_kv expected_frames_per_iteration "$frames_per_iteration"
  manifest_kv expected_checksum "$checksum"
  manifest_kv o3yvbench_3dsx "$out_dir/o3yvbench.3dsx"
  manifest_kv o3yvbench_3dsx_bytes "$bench_bytes"
  manifest_kv o3yvbench_3dsx_sha256 "$bench_sha256"
  manifest_kv hardware_log_path "sdmc:/o3yvbench.log"
  manifest_kv hardware_log_copy "$out_dir/old3ds-bench.log"
  manifest_kv verify_bench_command "$verify_bench_command"
  manifest_kv verify_playback_command "$verify_playback_command"
  manifest_kv playability_report_command "$report_command"
  manifest_kv azahar_timing_command "$azahar_timing_command"
  manifest_kv azahar_visual_smoke_command "$azahar_visual_command"
  manifest_kv azahar_config_command "$azahar_config_command"
  manifest_kv azahar_repeat_command "$azahar_repeat_command"
  manifest_kv bundle_status_command "$bundle_status_command"
  manifest_kv sd_detect_command "$sd_detect_command"
  manifest_kv sd_install_command "$sd_install_command"
  manifest_kv sd_import_log_command "$sd_import_log_command"
} >"$out_dir/MANIFEST.env"

cat >"$out_dir/RUN_ON_OLD3DS.txt" <<EOF
Copy o3yvbench.3dsx to the Old3DS SD card and launch it from the Homebrew
Launcher. The harness first writes decoder benchmark timing, then starts
24 fps top-screen Y2R playback. Wait for playback to start before exiting.

To copy the app to a physical Old3DS SD-card root from the host:

  $sd_detect_command
  $sd_install_command

The machine-readable log is written to:

  sdmc:/o3yvbench.log

Bundle metadata is in:

  $out_dir/MANIFEST.env

After the run, copy that log back beside this bundle as old3ds-bench.log and
verify timing/output determinism from the repository root with:

  $sd_import_log_command

  $verify_bench_command

For playback timing, inspect:

  $verify_playback_command

For one combined playability report:

  $report_command

For one bundle-level status after Azahar or hardware logs are present:

  $bundle_status_command

Strict copy-output target:

  bench_result worst_us <= $target_us

Direct-plane playback target:

  direct_bench_result worst_us <= $target_us
  playback worst_work_us <= 41666 and late_frames == 0
EOF

echo "wrote Old3DS run bundle: $out_dir"
cat "$out_dir/o3yvbench.3dsx.sha256"
