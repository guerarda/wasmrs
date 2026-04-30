#!/bin/sh
# Encode a wast directive (via wast_debug --no-run) and drop into rust-lldb
# on wasmrs with the .wasm queued as the program argument. At the (lldb)
# prompt, set breakpoints (e.g. `b file.rs:N`) then `run`.
set -e

if [ $# -ne 2 ]; then
    echo "usage: $0 <file.wast> <test-number>" >&2
    exit 2
fi

wast="$1"
n="$2"
repo=$(cd "$(dirname "$0")/.." && pwd)
cd "$repo"

cargo run --quiet --bin wast_debug --features spec-tests -- \
    --no-run "$wast" "$n"

stem=$(basename "$wast" .wast)
cache="$repo/target/wast-debug/$stem-$n.wasm"

cargo build --quiet --bin wasmrs
exec rust-lldb -- "$repo/target/debug/wasmrs" "$cache"
