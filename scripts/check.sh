#!/usr/bin/env sh
set -eu

cargo fmt --all -- --check
cargo check -Z build-std=core,compiler_builtins
