#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
OUT="${ROOT}/apps/desktop/src-tauri/linux"

(
  cd "${ROOT}"
  cargo build -p afs-cli -p afsd -p afs-fuse --release
)

mkdir -p "${OUT}"
cp "${ROOT}/target/release/afs" "${OUT}/afs"
cp "${ROOT}/target/release/afsd" "${OUT}/afsd"
cp "${ROOT}/target/release/afs-fuse" "${OUT}/afs-fuse"
chmod 755 "${OUT}/afs" "${OUT}/afsd" "${OUT}/afs-fuse"

echo "Prepared Linux CLI in ${OUT}/afs"
echo "Prepared Linux daemon in ${OUT}/afsd"
echo "Prepared Linux FUSE helper in ${OUT}/afs-fuse"
