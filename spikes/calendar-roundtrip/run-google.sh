#!/usr/bin/env bash
# RUN-20261124-hwkxry for EXP-20261123-bhfzfs: every corpus case through the Google
# Calendar API v3 (events.import into a throwaway calendar): write, read back and compare;
# edit as nitya would, write back, read again and compare. The calendar nitya-spike is
# made for the run and deleted at its end.
# Needs the owner's tokens from nitya-consent, decrypted by agenix into $NITYA_SECRETS
# (default ~/.config/nitya). From the repo root:
#   nix develop .#calendar-spike -c spikes/calendar-roundtrip/run-google.sh
set -euo pipefail
here=spikes/calendar-roundtrip
out=${YAJNA_RUN_DIR:-$here/out/google}
rm -rf "${out:?}" && mkdir -p "$out"
cargo build -q --locked --manifest-path "$here/Cargo.toml"
python3 -I "$here/providers.py" google "$here/target/debug/calendar-roundtrip" "$here/corpus" "$out"
