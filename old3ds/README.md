# O3YV Old3DS Timing Harness

This directory contains the hardware timing and playback path for the Rust O3YV
decoder. It builds a `.3dsx` that embeds a representative O3YV stream as a
binary asset, calls the Rust decoder through a C ABI, copies each decoded frame
into two reusable YUV420P eye buffers, reports total and worst-frame decode time
on hardware, then plays the stream on the top stereo screen.

The local qemu ARM Linux gate is still useful for regressions, but the project
goal is only proven by running this harness on an actual Old3DS.

## Prerequisites

- devkitPro with devkitARM and libctru
- `3dsxtool` in `PATH`
- Rust nightly with `rust-src`
- A representative stream, normally `tmp/reencode_lazy128_current.o3yv`

Install the Rust pieces with:

```sh
rustup toolchain install nightly
rustup component add rust-src --toolchain nightly
```

If devkitPro is not installed system-wide, the local no-sudo fallback is:

```sh
tools/o3yv-old3ds-fetch-devkitpro-image.sh
export DEVKITPRO="/tmp/o3yv-devkitpro-root/opt/devkitpro"
export DEVKITARM="$DEVKITPRO/devkitARM"
export PATH="$DEVKITPRO/tools/bin:$DEVKITARM/bin:$PATH"
```

## Candidate Build

The current emulator-tested playback candidate uses a shorter raw-frame gap
(`--keyint 16`) to keep the direct-plane Y2R playback path under the 15 ms
decode target on the heaviest frames:

```sh
tools/o3yv-old3ds-build-playable-candidate.sh \
  /path/to/800x240-sbs-yuv420.yuv \
  tmp/reencode_old3ds_directpass.o3yv \
  tmp/o3yv-old3ds-playable
```

Then run the Azahar repeat and visual checks from `MANIFEST.env` before copying
the bundle to hardware.

To copy the packaged `.3dsx` to a physical Old3DS SD-card root and later import
the hardware log back into the bundle:

```sh
tools/o3yv-old3ds-sd-handoff.sh detect
tools/o3yv-old3ds-sd-handoff.sh install tmp/o3yv-old3ds-playable /media/$USER/OLD3DS
tools/o3yv-old3ds-sd-handoff.sh import-log tmp/o3yv-old3ds-playable /media/$USER/OLD3DS
```

## Build

From the repository root:

```sh
tools/o3yv-old3ds-build-harness.sh tmp/reencode_lazy128_current.o3yv 8 15000
```

The script writes `old3ds/generated/o3yv_stream.{h,s}` and builds
`old3ds/build/o3yvbench.3dsx`. The generated header also embeds the expected
frame count and decoded-output checksum for the selected stream.

To build the harness and package the SD-card run files together:

```sh
tools/o3yv-old3ds-package-run.sh
```

The run bundle includes `MANIFEST.env`, a sourceable key/value manifest with
the `.3dsx` SHA-256, stream checksum, frame count, target budgets, and exact
verification commands for hardware and Azahar evidence.

## Run

Launch `o3yvbench.3dsx` on an Old3DS. The harness prints to the bottom-screen
console and writes the same benchmark output to `sdmc:/o3yvbench.log`:

- frame count
- iteration count
- min, mean, median, and p95 milliseconds per decoded/output frame
- split mean/worst decode and YUV420P output-copy timings
- worst single-frame milliseconds
- top frames by worst observed decode/output time
- a `bench_result ...` line for machine checking
- a `direct_bench_result ...` line for the direct YUV plane path used by Y2R
- expected and measured decoded-output checksums
- a `playback_result ...` line for the first rendered playback pass
- error code, if decoding fails

After the benchmark, the top screen plays the embedded stereo stream at 24 fps
using the 3DS Y2R hardware converter, with a slow software BGR8 renderer only
as a fallback. Press START to exit.

The strict copy-output budget uses `bench_result worst_us <= 15 ms`. The
direct-plane playback budget uses `direct_bench_result worst_us <= 15 ms` plus
`playback_result late_frames=0`; this is the path used by Y2R playback.

After the run, copy `sdmc:/o3yvbench.log` back to the host. For a packaged
bundle, the handoff helper imports the log to the manifest-defined
`old3ds-bench.log` path and runs the bundle status verifier:

```sh
tools/o3yv-old3ds-sd-handoff.sh import-log \
  tmp/o3yv-old3ds-playable /media/$USER/OLD3DS
```

For a standalone log path, check it with:

```sh

tools/o3yv-old3ds-verify-log.sh \
  old3ds-bench.log tmp/reencode_lazy128_current.o3yv 8 15000
```

The strict verifier above checks the conservative decoder budget. To validate
the rendered 24 fps playback pass separately:

```sh
tools/o3yv-old3ds-check-playback-log.sh \
  old3ds-bench.log 41666 100 24 y2r direct_planes
```

For a combined report that keeps the strict decoder result and playback result
separate:

```sh
tools/o3yv-old3ds-playability-report.sh \
  old3ds-bench.log tmp/reencode_lazy128_current.o3yv 8 15000 41666 real_old3ds
```

## Azahar Smoke Test

With Azahar installed at `/opt/3ds/azahar` and `DISPLAY=:0`, the host can run
an emulator timing pass and a visual smoke check:

```sh
tools/o3yv-azahar-run-bench.sh \
  tmp/o3yv-old3ds-playable/o3yvbench.3dsx \
  tmp/azahar-old3ds-y2r-playback.log 120 playback_result

tools/o3yv-old3ds-playability-report.sh \
  tmp/azahar-old3ds-y2r-playback.log \
  tmp/reencode_lazy128_current.o3yv 8 15000 41666 azahar_old3ds

tools/o3yv-azahar-repeat-bench.sh \
  tmp/o3yv-old3ds-playable/o3yvbench.3dsx \
  tmp/azahar-repeat-bench 3 120 \
  tmp/reencode_lazy128_current.o3yv 8 15000 41666

tools/o3yv-azahar-visual-smoke.sh \
  tmp/o3yv-old3ds-playable/o3yvbench.3dsx \
  tmp/azahar-visual-smoke 120

tools/o3yv-old3ds-bundle-status.sh tmp/o3yv-old3ds-playable
```

Azahar evidence can only support `playability_status=plausible`; final proof
requires the same report against a real Old3DS `sdmc:/o3yvbench.log`.
