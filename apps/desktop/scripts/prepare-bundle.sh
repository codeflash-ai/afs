#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

case "$(uname -s)" in
  Darwin)
    exec "${SCRIPT_DIR}/prepare-macos-file-provider.sh"
    ;;
  Linux)
    exec "${SCRIPT_DIR}/prepare-linux-bundle.sh"
    ;;
  *)
    printf 'prepare-bundle: unsupported OS: %s\n' "$(uname -s)" >&2
    exit 1
    ;;
esac
