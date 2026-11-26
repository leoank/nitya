#!/usr/bin/env bash
# RUN-20261126-fexqm6 for EXP-20261123-bhfzeh: the key-hierarchy spike's scenarios as
# tests (offline sync with a calendar-only delegate, the signed log and tampering,
# revocation by re-wrapping only and by rotating the DEKs, recovery from the passphrase,
# associated data), then the timings over 100k items of 2 KiB in release mode.
# From the repo root:
#   nix develop -c spikes/key-hierarchy/run.sh
set -euo pipefail
here=spikes/key-hierarchy
out=${YAJNA_RUN_DIR:-$here/out}
rm -rf "${out:?}" && mkdir -p "$out"
{ uname -sm; rustc --version; } > "$out/host.txt"
cargo test --locked --release --manifest-path "$here/Cargo.toml" -- --test-threads=1 2>&1 \
  | grep -E '^test |^test result' | tee "$out/tests.txt"
cargo run --locked --release --manifest-path "$here/Cargo.toml" -q -- 100000 2048 | tee "$out/timings.txt"
