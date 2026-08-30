#!/usr/bin/env bash
# Fetch Firecracker v1.16.1 + CI kernel into ./images
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
IMG="${ROOT}/images"
ARCH="$(uname -m)"
FC_VER="v1.16.1"
S3="https://s3.amazonaws.com/spec.ccfc.min"
CI_VERSION="${FC_VER%.*}"

mkdir -p "$IMG"
cd "$IMG"

s3_xml() {
  curl -fsSL "${S3}?list-type=2&prefix=${1}"
}

# Keys only (no .config, no -no-acpi). Empty grep must not abort the script.
kernel_keys() {
  grep -oE 'firecracker-ci/[^<[:space:]]+/vmlinux-[0-9]+\.[0-9]+\.[0-9]+' || true
}

# Firecracker 1.16 CI still ships 5.10 + 6.1. Prefer 6.1; skip 6.18+.
pick_kernel() {
  local keys="$1" picked=""
  picked=$(printf '%s\n' "$keys" | grep 'vmlinux-6\.1\.' | sort -V | tail -1 || true)
  if [[ -z "$picked" ]]; then
    picked=$(printf '%s\n' "$keys" | grep 'vmlinux-5\.10\.' | sort -V | tail -1 || true)
  fi
  printf '%s' "$picked"
}

if [[ ! -x firecracker ]]; then
  echo "fetching firecracker ${FC_VER}..."
  curl -fsSL -o fc.tgz \
    "https://github.com/firecracker-microvm/firecracker/releases/download/${FC_VER}/firecracker-${FC_VER}-${ARCH}.tgz"
  tar -xzf fc.tgz
  src=$(find . -maxdepth 2 -type f -name "firecracker-${FC_VER}-${ARCH}" | head -1)
  jail=$(find . -maxdepth 2 -type f -name "jailer-${FC_VER}-${ARCH}" | head -1)
  cp "$src" firecracker
  cp "$jail" jailer
  chmod +x firecracker jailer
  rm -rf fc.tgz release-"${FC_VER}"-"${ARCH}"
fi

if [[ ! -f vmlinux.bin ]]; then
  echo "listing Firecracker CI kernels..."
  keys=""
  # Current bucket layout: firecracker-ci/YYYYMMDD-<sha>-0/  (v1.16/ is empty)
  prefix=$(s3_xml "firecracker-ci/" | grep -oE "firecracker-ci/[0-9]{8}-[^/<]+/" | sort | tail -1 || true)
  if [[ -n "$prefix" ]]; then
    echo "dated prefix ${prefix}"
    keys=$(s3_xml "${prefix}${ARCH}/vmlinux-" | kernel_keys)
  fi
  if [[ -z "$keys" ]]; then
    echo "trying firecracker-ci/${CI_VERSION}/${ARCH}/"
    keys=$(s3_xml "firecracker-ci/${CI_VERSION}/${ARCH}/vmlinux-" | kernel_keys)
  fi
  key=$(pick_kernel "$keys")
  [[ -n "$key" ]] || { echo "no kernel key found"; echo "$keys"; exit 1; }
  echo "downloading ${key}"
  curl -fL --progress-bar -o vmlinux.bin "${S3}/${key}"
  [[ -s vmlinux.bin ]] || { echo "kernel download empty"; rm -f vmlinux.bin; exit 1; }
fi

echo "ok: $IMG/firecracker $IMG/jailer $IMG/vmlinux.bin"
ls -lh firecracker jailer vmlinux.bin
./firecracker --version || true
