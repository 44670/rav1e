# O3YV Old3DS Timing Harness

This directory contains the hardware timing path for the Rust O3YV decoder.
It builds a `.3dsx` that embeds a representative O3YV stream as a binary
asset, calls the Rust decoder through a C ABI, copies each decoded frame into
two reusable YUV420P eye buffers, and reports total and worst-frame decode time
on hardware.

The local qemu ARM Linux gate is still useful for regressions, but the project
goal is only proven by running this harness on an actual Old3DS.

## Prerequisites

- devkitPro with devkitARM and libctru
- `makerom` and `3dsxtool` in `PATH`
- Rust nightly with `rust-src`
- A representative stream, normally `tmp/reencode_lazy128_current.o3yv`

Install the Rust pieces with:

```sh
rustup toolchain install nightly
rustup component add rust-src --toolchain nightly
```

## Build

From the repository root:

```sh
tools/o3yv-old3ds-build-harness.sh tmp/reencode_lazy128_current.o3yv
```

The script writes `old3ds/generated/o3yv_stream.{h,s}` and builds
`old3ds/build/o3yvbench.3dsx`.

## Run

Launch `o3yvbench.3dsx` on an Old3DS. The harness prints:

- frame count
- iteration count
- mean milliseconds per decoded/output frame
- worst single-frame milliseconds
- a `bench_result ...` line for machine checking
- error code, if decoding fails

Passing the project performance target requires worst-frame timing below
`15 ms` for the representative 800x240 SBS stream on Old3DS hardware.

Captured logs can be checked on the host with:

```sh
tools/o3yv-old3ds-check-log.sh old3ds-bench.log
```
