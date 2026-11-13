#!/bin/sh
set -eu

cd "$(dirname "$0")"
exec cargo run --release --locked --offline --bin ptyra -- --install
