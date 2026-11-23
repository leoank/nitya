#!/usr/bin/env bash
# EXP-20261123-bhfzfs: round-trip every corpus case through a local Radicale (CalDAV):
# PUT, GET and compare; edit what came back, PUT with If-Match, GET and compare again.
# Radicale runs on loopback with no auth and a fresh storage folder each time.
# From the repo root: nix develop .#calendar-spike -c spikes/calendar-roundtrip/run-caldav-radicale.sh
set -euo pipefail
here=spikes/calendar-roundtrip
out=${YAJNA_RUN_DIR:-$here/out/caldav-radicale}
port=${RADICALE_PORT:-5299}
rm -rf "$out" && mkdir -p "$out"
store=$(mktemp -d)
cargo build -q --locked --manifest-path "$here/Cargo.toml"
radicale --version > "$out/radicale-version.txt"
radicale --storage-filesystem-folder "$store" --auth-type none \
  --server-hosts "127.0.0.1:$port" > "$out/radicale.log" 2>&1 &
pid=$!
trap 'kill $pid 2>/dev/null; rm -rf "$store"' EXIT
for _ in $(seq 100); do curl -s -o /dev/null "http://127.0.0.1:$port/" && break; sleep 0.1; done
"$here/target/debug/calendar-roundtrip" caldav "http://127.0.0.1:$port/nitya/exp/" "$here/corpus" "$out" \
  | tee "$out/table.tsv"
