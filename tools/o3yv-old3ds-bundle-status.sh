#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'USAGE'
usage: tools/o3yv-old3ds-bundle-status.sh [bundle_dir]

Audits an Old3DS run bundle produced by tools/o3yv-old3ds-package-run.sh.
It checks the packaged .3dsx hash/size, summarizes optional Azahar repeat and
visual-smoke evidence, and uses a copied real Old3DS log when present.

Status meanings:
  pass_hardware  Real Old3DS log proves deterministic output, direct-plane timing,
                 and 24 fps playback.
  plausible      Artifact is intact and Azahar direct-plane repeat+visual evidence pass.
  needs_evidence More timing/visual evidence is required.
  fail           Bundle artifact or provided evidence failed validation.

Defaults:
  bundle_dir  tmp/o3yv-old3ds-playable
USAGE
}

if [[ "${1:-}" == "-h" || "${1:-}" == "--help" ]]; then
  usage
  exit 0
fi

bundle_dir=${1:-tmp/o3yv-old3ds-playable}
manifest="$bundle_dir/MANIFEST.env"

if [[ ! -f "$manifest" ]]; then
  echo "missing bundle manifest: $manifest" >&2
  exit 1
fi

# MANIFEST.env is generated with printf %q specifically to be sourceable.
# shellcheck disable=SC1090
source "$manifest"

required_vars=(
  repo_commit
  input_stream
  iterations
  bench_target_us
  playback_target_us
  expected_checksum
  o3yvbench_3dsx
  o3yvbench_3dsx_bytes
  o3yvbench_3dsx_sha256
  hardware_log_path
  hardware_log_copy
)
for name in "${required_vars[@]}"; do
  if [[ -z "${!name:-}" ]]; then
    echo "missing $name in $manifest" >&2
    exit 1
  fi
done

kv_value() {
  local line=$1
  local key=$2
  awk -v key="$key" '
    {
      for (i = 1; i <= NF; i++) {
        split($i, kv, "=")
        if (kv[1] == key) {
          print kv[2]
          exit
        }
      }
    }
  ' <<<"$line"
}

bool_status() {
  local status=$1
  case "$status" in
    pass | strict_pass | plausible | pass_hardware)
      return 0
      ;;
    *)
      return 1
      ;;
  esac
}

artifact_status=fail
actual_sha256=missing
actual_bytes=missing
if [[ -f "$o3yvbench_3dsx" ]]; then
  actual_sha256=$(sha256sum "$o3yvbench_3dsx" | awk '{ print $1; exit }')
  actual_bytes=$(wc -c <"$o3yvbench_3dsx")
  if [[ "$actual_sha256" == "$o3yvbench_3dsx_sha256" ]] \
    && [[ "$actual_bytes" == "$o3yvbench_3dsx_bytes" ]]; then
    artifact_status=pass
  fi
fi

printf 'bundle_artifact status=%s bundle_dir=%s repo_commit=%s input_stream=%s expected_sha256=%s actual_sha256=%s expected_bytes=%s actual_bytes=%s\n' \
  "$artifact_status" "$bundle_dir" "$repo_commit" "$input_stream" \
  "$o3yvbench_3dsx_sha256" "$actual_sha256" \
  "$o3yvbench_3dsx_bytes" "$actual_bytes"

repeat_file="$bundle_dir/azahar-repeat-bench/summary.txt"
repeat_status=missing
repeat_checksum=unknown
repeat_checksum_status=unknown
if [[ -f "$repeat_file" ]]; then
  repeat_line=$(grep '^azahar_repeat_summary ' "$repeat_file" | tail -1 || true)
  if [[ -n "$repeat_line" ]]; then
    repeat_status=$(kv_value "$repeat_line" status)
    repeat_runs=$(kv_value "$repeat_line" runs)
    repeat_playback_pass=$(kv_value "$repeat_line" playback_pass)
    repeat_bench_output_pass=$(kv_value "$repeat_line" bench_output_pass)
    repeat_bench_timing_pass=$(kv_value "$repeat_line" bench_timing_pass)
    repeat_direct_timing_pass=$(kv_value "$repeat_line" direct_timing_pass)
    repeat_max_bench_worst_us=$(kv_value "$repeat_line" max_bench_worst_us)
    repeat_max_direct_worst_us=$(kv_value "$repeat_line" max_direct_worst_us)
    repeat_max_playback_worst_work_us=$(
      kv_value "$repeat_line" max_playback_worst_work_us
    )
    repeat_max_late_frames=$(kv_value "$repeat_line" max_late_frames)
    repeat_checksum_status=$(kv_value "$repeat_line" checksum_status)
    repeat_checksum=$(kv_value "$repeat_line" checksum)
    if [[ "${repeat_checksum,,}" != "${expected_checksum,,}" ]]; then
      repeat_status=stale
    fi
    printf 'azahar_repeat status=%s runs=%s playback_pass=%s bench_output_pass=%s bench_timing_pass=%s direct_timing_pass=%s max_bench_worst_us=%s max_direct_worst_us=%s max_playback_worst_work_us=%s max_late_frames=%s checksum_status=%s checksum=%s expected_checksum=%s summary=%s\n' \
      "$repeat_status" "${repeat_runs:-unknown}" \
      "${repeat_playback_pass:-unknown}" \
      "${repeat_bench_output_pass:-unknown}" \
      "${repeat_bench_timing_pass:-unknown}" \
      "${repeat_direct_timing_pass:-unknown}" \
      "${repeat_max_bench_worst_us:-unknown}" \
      "${repeat_max_direct_worst_us:-unknown}" \
      "${repeat_max_playback_worst_work_us:-unknown}" \
      "${repeat_max_late_frames:-unknown}" \
      "${repeat_checksum_status:-unknown}" "${repeat_checksum:-unknown}" \
      "$expected_checksum" "$repeat_file"
  else
    repeat_status=malformed
    printf 'azahar_repeat status=malformed summary=%s\n' "$repeat_file"
  fi
else
  printf 'azahar_repeat status=missing summary=%s\n' "$repeat_file"
fi

visual_file="$bundle_dir/azahar-visual-smoke/visual-smoke.txt"
visual_log="$bundle_dir/azahar-visual-smoke/o3yvbench.log"
visual_status=missing
visual_checksum=unknown
if [[ -f "$visual_file" ]]; then
  visual_line=$(grep '^azahar_visual_smoke ' "$visual_file" | tail -1 || true)
  diff_line=$(grep '^diff ' "$visual_file" | tail -1 || true)
  if [[ -n "$visual_line" ]]; then
    visual_status=$(kv_value "$visual_line" status)
    visual_ae=$(kv_value "$diff_line" ae)
    visual_rmse_normalized=$(kv_value "$diff_line" rmse_normalized)
    if [[ -f "$visual_log" ]]; then
      visual_bench_line=$(
        grep '^bench_result ' "$visual_log" | tail -1 || true
      )
      visual_checksum=$(kv_value "$visual_bench_line" checksum)
      if [[ "${visual_checksum,,}" != "${expected_checksum,,}" ]]; then
        visual_status=stale
      fi
    else
      visual_status=stale
    fi
    printf 'azahar_visual status=%s ae=%s rmse_normalized=%s checksum=%s expected_checksum=%s report=%s log=%s\n' \
      "$visual_status" "${visual_ae:-unknown}" \
      "${visual_rmse_normalized:-unknown}" "${visual_checksum:-unknown}" \
      "$expected_checksum" "$visual_file" "$visual_log"
  else
    visual_status=malformed
    printf 'azahar_visual status=malformed report=%s\n' "$visual_file"
  fi
else
  printf 'azahar_visual status=missing report=%s\n' "$visual_file"
fi

hardware_status=missing
hardware_report_rc=0
hardware_bench_timing=unknown
hardware_direct_bench=unknown
hardware_playback=unknown
hardware_notes=none
if [[ -f "$hardware_log_copy" ]]; then
  set +e
  hardware_report=$(
    tools/o3yv-old3ds-playability-report.sh \
      "$hardware_log_copy" "$input_stream" "$iterations" \
      "$bench_target_us" "$playback_target_us" real_old3ds 2>&1
  )
  hardware_report_rc=$?
  set -e
  hardware_status=$(
    awk -F= '/^playability_status=/ { print $2; exit }' \
      <<<"$hardware_report"
  )
  hardware_status=${hardware_status:-fail}
  hardware_bench_line=$(grep '^bench_timing ' <<<"$hardware_report" | tail -1 || true)
  hardware_direct_line=$(grep '^direct_bench ' <<<"$hardware_report" | tail -1 || true)
  hardware_playback_line=$(grep '^playback ' <<<"$hardware_report" | tail -1 || true)
  hardware_bench_timing=$(
    kv_value "$hardware_bench_line" status
  )
  hardware_direct_bench=$(
    kv_value "$hardware_direct_line" status
  )
  hardware_playback=$(
    kv_value "$hardware_playback_line" status
  )
  hardware_bench_timing=${hardware_bench_timing:-unknown}
  hardware_direct_bench=${hardware_direct_bench:-unknown}
  hardware_playback=${hardware_playback:-unknown}
  hardware_notes=$(
    awk -F= '/^note=/ { printf "%s%s", sep, $2; sep="," }' \
      <<<"$hardware_report"
  )
  hardware_notes=${hardware_notes:-none}
fi

printf 'hardware_report status=%s rc=%s log_copy=%s hardware_path=%s bench_timing=%s direct_bench=%s playback=%s notes=%s\n' \
  "$hardware_status" "$hardware_report_rc" "$hardware_log_copy" \
  "$hardware_log_path" "$hardware_bench_timing" "$hardware_direct_bench" \
  "$hardware_playback" "$hardware_notes"

overall_status=needs_evidence
if [[ "$artifact_status" != "pass" ]]; then
  overall_status=fail
elif [[ "$hardware_status" == "pass_hardware" ]]; then
  overall_status=pass_hardware
elif [[ "$hardware_status" != "missing" ]]; then
  overall_status=fail
elif bool_status "$repeat_status" && [[ "$visual_status" == "pass" ]]; then
  overall_status=plausible
fi

printf 'old3ds_bundle_status status=%s artifact=%s hardware=%s azahar_repeat=%s azahar_visual=%s\n' \
  "$overall_status" "$artifact_status" "$hardware_status" \
  "$repeat_status" "$visual_status"

case "$overall_status" in
  pass_hardware)
    printf 'note=real_old3ds_log_proves_playback\n'
    ;;
  plausible)
    printf 'note=azahar_evidence_only_needs_real_old3ds_log_for_final_proof\n'
    ;;
  needs_evidence)
    printf 'note=run_azahar_repeat_and_visual_or_copy_real_old3ds_log\n'
    ;;
  fail)
    printf 'note=bundle_or_evidence_failed_validation\n'
    ;;
esac

if [[ "$overall_status" == "fail" || "$overall_status" == "needs_evidence" ]]; then
  exit 1
fi
