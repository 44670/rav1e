#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'USAGE'
usage: tools/o3yv-old3ds-fetch-devkitpro-image.sh [root] [tag]

Fetches and extracts the amd64 devkitPro/devkitarm OCI image without Docker.
This is a no-sudo fallback for building the Old3DS .3dsx harness on hosts
where devkitPro is not installed system-wide.

Defaults:
  root  /tmp/o3yv-devkitpro-root
  tag   latest

After it finishes, use the printed DEVKITPRO/DEVKITARM/PATH exports before
running tools/o3yv-old3ds-build-harness.sh.
USAGE
}

if [[ "${1:-}" == "-h" || "${1:-}" == "--help" ]]; then
  usage
  exit 0
fi

root=${1:-/tmp/o3yv-devkitpro-root}
tag=${2:-latest}
repo=devkitpro/devkitarm
registry=https://registry-1.docker.io
auth_url="https://auth.docker.io/token?service=registry.docker.io&scope=repository:${repo}:pull"
blob_cache=${O3YV_DKP_BLOB_CACHE:-/tmp/o3yv-devkitpro-blobs}

require_command() {
  local command=$1
  if ! command -v "$command" >/dev/null 2>&1; then
    echo "missing command: $command" >&2
    exit 1
  fi
}

docker_token() {
  curl -fsSL "$auth_url" \
    | sed -n 's/.*"token":"\([^"]*\)".*/\1/p'
}

amd64_manifest_digest() {
  awk '
    /"digest": "sha256:/ {
      digest = $0
      sub(/.*sha256:/, "", digest)
      sub(/".*/, "", digest)
    }
    /"architecture": "amd64"/ {
      print digest
      exit
    }
  '
}

layer_digests() {
  awk '
    /"layers"/ { layers = 1; next }
    layers && /\]/ { exit }
    layers && /"digest": "sha256:/ {
      digest = $0
      sub(/.*sha256:/, "", digest)
      sub(/".*/, "", digest)
      print digest
    }
  '
}

require_command curl
require_command sed
require_command awk
require_command tar

mkdir -p "$root" "$blob_cache"
token=$(docker_token)
if [[ -z "$token" ]]; then
  echo "failed to get Docker registry token" >&2
  exit 1
fi

index=$(
  curl -fsSL \
    -H "Authorization: Bearer $token" \
    -H "Accept: application/vnd.oci.image.index.v1+json, application/vnd.docker.distribution.manifest.list.v2+json" \
    "$registry/v2/$repo/manifests/$tag"
)
manifest_digest=$(printf '%s\n' "$index" | amd64_manifest_digest)
if [[ -z "$manifest_digest" ]]; then
  echo "failed to find linux/amd64 manifest for $repo:$tag" >&2
  exit 1
fi

manifest=$(
  curl -fsSL \
    -H "Authorization: Bearer $token" \
    -H "Accept: application/vnd.oci.image.manifest.v1+json, application/vnd.docker.distribution.manifest.v2+json" \
    "$registry/v2/$repo/manifests/sha256:$manifest_digest"
)

printf '%s\n' "$manifest" | layer_digests | while read -r digest; do
  layer="$blob_cache/$digest.tar.gz"
  if [[ ! -f "$layer" ]]; then
    echo "downloading sha256:$digest"
    curl -fL --retry 3 \
      -H "Authorization: Bearer $token" \
      -o "$layer" \
      "$registry/v2/$repo/blobs/sha256:$digest"
  else
    echo "using cached sha256:$digest"
  fi
  echo "extracting sha256:$digest"
  tar -xzf "$layer" -C "$root"
done

devkitpro="$root/opt/devkitpro"
devkitarm="$devkitpro/devkitARM"
if [[ ! -x "$devkitarm/bin/arm-none-eabi-gcc" ]]; then
  echo "missing extracted devkitARM compiler under $devkitarm" >&2
  exit 1
fi
if [[ ! -x "$devkitpro/tools/bin/3dsxtool" ]]; then
  echo "missing extracted 3dsxtool under $devkitpro/tools/bin" >&2
  exit 1
fi
if [[ ! -f "$devkitpro/libctru/include/3ds.h" ]]; then
  echo "missing extracted libctru headers under $devkitpro/libctru" >&2
  exit 1
fi

cat <<EOF
Extracted devkitPro image to $root

export DEVKITPRO="$devkitpro"
export DEVKITARM="$devkitarm"
export PATH="$devkitpro/tools/bin:$devkitarm/bin:\$PATH"
EOF
