#!/usr/bin/env bash
# Build a desktop bundle on its matching native host; CI supplies the other hosts.
set -euo pipefail
ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"
case "$(uname -s)-$(uname -m)" in
    Linux-x86_64) native=linux-x86_64; triple=x86_64-unknown-linux-gnu ;;
    Linux-aarch64|Linux-arm64) native=linux-aarch64; triple=aarch64-unknown-linux-gnu ;;
    Darwin-arm64) native=macos-aarch64; triple=aarch64-apple-darwin ;;
    Darwin-x86_64) native=macos-x86_64; triple=x86_64-apple-darwin ;;
    *) echo 'Unsupported native build host' >&2; exit 1 ;;
esac
requested="${1:-$native}"
if [[ "$requested" == macos-arm64 ]]; then requested=macos-aarch64; fi
if [[ "$requested" != "$native" ]]; then
    echo "This host builds $native. Use the matching CI runner for $requested." >&2
    exit 1
fi
cargo build --release --locked --target "$triple"
python3 tools/package_release.py --target "$triple"
