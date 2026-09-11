#!/bin/sh
set -eu
cd -- "$(dirname -- "$0")"
if [ -f ./phantom-studio ] && [ -x ./phantom-studio ]; then
    exec ./phantom-studio "$@"
fi
if [ -x target/release/phantom-studio ]; then
    exec target/release/phantom-studio "$@"
fi
exec cargo run --locked -- "$@"
