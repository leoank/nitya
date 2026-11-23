#!/usr/bin/env bash
# EXP-20261123-bhfzfs: parse and re-serialize every corpus case with each candidate
# iCalendar crate, and compare meaning (instances over two years) and raw properties.
# From the repo root: nix develop .#calendar-spike -c spikes/calendar-roundtrip/run-crates.sh
set -euo pipefail
here=spikes/calendar-roundtrip
out=${YAJNA_RUN_DIR:-$here/out/crates}
rm -rf "$out" && mkdir -p "$out"
cargo build -q --locked --manifest-path "$here/Cargo.toml"
"$here/target/debug/calendar-roundtrip" crates "$here/corpus" "$out" | tee "$out/table.tsv"
