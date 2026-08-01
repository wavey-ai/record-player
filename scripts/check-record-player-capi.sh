#!/bin/sh
set -eu

cargo test --locked -p record-player-capi
cargo build --locked --release -p record-player-capi
"${CC:-cc}" -std=c11 -x c-header -fsyntax-only include/record_player.h
"${CXX:-c++}" -std=c++17 -x c++-header -fsyntax-only include/record_player.h
"${CC:-cc}" -std=c11 -Iinclude \
  crates/record-player-capi/tests/capi_smoke.c \
  target/release/librecord_player_capi.a \
  -o target/record-player-capi-smoke
target/record-player-capi-smoke
