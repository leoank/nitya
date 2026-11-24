#!/usr/bin/env bash
# RUN-20261124-hwkyzz for EXP-20261123-bhfzfs: every corpus case through Microsoft Graph
# v1.0 (a throwaway calendar): write, read back and compare; edit as nitya would, write
# back, read again and compare. Case 18 is rewritten so that the owner's Microsoft account
# invites the owner's Google account; the run then accepts from Google and waits for the
# answer in Graph (prediction 4). The calendar nitya-spike is made for the run and deleted
# at its end, its events first, which cancels the meeting for the Google account.
# Needs the owner's tokens from nitya-consent, decrypted by agenix into $NITYA_SECRETS
# (default ~/.config/nitya). From the repo root:
#   nix develop .#calendar-spike -c spikes/calendar-roundtrip/run-graph.sh
set -euo pipefail
here=spikes/calendar-roundtrip
out=${YAJNA_RUN_DIR:-$here/out/graph}
rm -rf "${out:?}" && mkdir -p "$out"
cargo build -q --locked --manifest-path "$here/Cargo.toml"
python3 -I "$here/providers.py" graph "$here/target/debug/calendar-roundtrip" "$here/corpus" "$out"
