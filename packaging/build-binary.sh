#!/bin/sh
# Builds tad for one target, reproducibly (paths remapped, no incremental state). Called by the package
# conveyor: packaging/build-binary.sh TARGET
set -eu
target="${1:?usage: $0 <rust target triple, e.g. x86_64-unknown-linux-musl>}"
cd "$(dirname "$0")/.."
root="$PWD"
cargo_home="${CARGO_HOME:-$HOME/.cargo}"
sysroot="$(rustc --print sysroot)"
export RUSTFLAGS="${RUSTFLAGS:-} --remap-path-prefix=$root=/build --remap-path-prefix=$cargo_home=/cargo --remap-path-prefix=$sysroot=/rust"
export CARGO_INCREMENTAL=0
cargo build --release --locked --target "$target" -p tad
echo "built target/$target/release/tad with $(rustc -V)"
