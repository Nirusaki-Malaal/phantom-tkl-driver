#!/bin/sh
set -eu
if ! command -v python3 >/dev/null 2>&1; then
    echo "Please install Python 3 with your distribution's package manager, then run this script again." >&2
    exit 1
fi
exec python3 "$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)/tools/install.py" "$@"
