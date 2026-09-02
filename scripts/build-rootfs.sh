#!/usr/bin/env bash
# Cratera Rootfs Builder (Driven by languages.toml Manifest)
#
# 1. Dynamically renders Dockerfile.rootfs from languages.toml
# 2. Builds container image via Docker / Podman (with layer caching)
# 3. Exports complete filesystem directly into images/rootfs.ext4
#
# Supports both Docker and Podman (rootless).

set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
IMG="${ROOT}/images"
LANG_FILE="${LANGUAGES_FILE:-${ROOT}/languages.toml}"
DOCKERFILE="${ROOT}/Dockerfile.rootfs"
GENERATOR="${ROOT}/scripts/generate-dockerfile.sh"

mkdir -p "$IMG"

if [[ ! -f "$LANG_FILE" ]]; then
  echo "ERROR: Manifest not found at $LANG_FILE"
  exit 1
fi

# Detect Container Engine (Docker or Podman)
CONTAINER_ENGINE="${CONTAINER_ENGINE:-}"
if [[ -z "$CONTAINER_ENGINE" ]]; then
  if command -v docker >/dev/null 2>&1 && docker info >/dev/null 2>&1; then
    CONTAINER_ENGINE="docker"
  elif command -v podman >/dev/null 2>&1 && podman info >/dev/null 2>&1; then
    CONTAINER_ENGINE="podman"
  else
    echo "ERROR: Neither Docker nor Podman is available."
    exit 1
  fi
fi

LANG_FILTER="${LANGUAGES:-all}"
LANG_FILTER="$(echo "$LANG_FILTER" | tr '[:upper:]' '[:lower:]' | tr -d ' ')"
IMAGE_TAG="cratera-rootfs:latest"

echo "======================================================="
echo " Cratera Rootfs Builder"
echo " Manifest:         $LANG_FILE"
echo " Active Preset:    $LANG_FILTER"
echo " Container Engine: $CONTAINER_ENGINE"
echo "======================================================="

echo "==> [1/4] Generating Dockerfile.rootfs from manifest..."
"$GENERATOR" "$LANG_FILE" "$DOCKERFILE" "$LANG_FILTER"

echo "==> [2/4] Building container image via $CONTAINER_ENGINE..."
$CONTAINER_ENGINE build -t "$IMAGE_TAG" -f "$DOCKERFILE" "$ROOT"

echo "==> [3/4] Exporting root filesystem from container..."
CONTAINER_ID=$($CONTAINER_ENGINE create "$IMAGE_TAG")
mkdir -p "${ROOT}/target"
STAGING=$(mktemp -d -p "${ROOT}/target" "cratera-staging.XXXXXX")

cleanup() {
  $CONTAINER_ENGINE rm -f "${CONTAINER_ID:-}" >/dev/null 2>&1 || true
  rm -rf "${STAGING:-}" >/dev/null 2>&1 || true
}
trap cleanup EXIT

$CONTAINER_ENGINE export "$CONTAINER_ID" | tar -xf - -C "$STAGING"

# Ensure essential directories and permissions
mkdir -p "$STAGING"/{proc,sys,dev,tmp,run,dev/shm,root}
chmod 1777 "$STAGING"/tmp "$STAGING"/dev/shm
chmod 700 "$STAGING"/root

USED_MB=$(du -sm "$STAGING" | awk '{print $1}')

if command -v mksquashfs >/dev/null 2>&1; then
  ROOTFS_OUT="${IMG}/rootfs.squashfs"
  echo "==> [4/4] Creating SquashFS image at $ROOTFS_OUT (uncompressed payload: ${USED_MB}M)..."
  rm -f "$ROOTFS_OUT"
  mksquashfs "$STAGING" "$ROOTFS_OUT" -comp zstd -Xcompression-level 3 -noappend -processors "$(nproc)" -quiet
  # Maintain backwards-compatible ext4 link
  ln -sf "rootfs.squashfs" "${IMG}/rootfs.ext4" 2>/dev/null || true
else
  ROOTFS_OUT="${IMG}/rootfs.ext4"
  AUTO_SIZE_MB=$(( (USED_MB * 13 / 10) + 512 ))
  if [[ "$AUTO_SIZE_MB" -lt 4096 ]]; then
    AUTO_SIZE_MB=4096
  fi
  FINAL_SIZE_MB="${ROOTFS_SIZE_MB:-$AUTO_SIZE_MB}"
  echo "==> [4/4] Creating sparse ${FINAL_SIZE_MB}M ext4 image at $ROOTFS_OUT (uncompressed payload: ${USED_MB}M)..."
  rm -f "$ROOTFS_OUT"
  truncate -s "${FINAL_SIZE_MB}M" "$ROOTFS_OUT"
  mkfs.ext4 -q -F -d "$STAGING" "$ROOTFS_OUT"
  tune2fs -m 0 "$ROOTFS_OUT" >/dev/null
fi

echo "==> Rootfs build complete: $ROOTFS_OUT ($(du -h "$ROOTFS_OUT" | awk '{print $1}') on disk)"
sha256sum "$ROOTFS_OUT" | awk '{print $1}' > "${ROOTFS_OUT}.sha256"
