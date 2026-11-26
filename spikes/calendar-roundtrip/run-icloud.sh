#!/usr/bin/env bash
# RUN-20261126-ej2zgp for EXP-20261123-bhfzfs: every corpus case through iCloud's CalDAV
# (one resource per UID in a throwaway calendar): PUT, GET and compare; edit as nitya
# would, PUT with If-Match, GET and compare again. iCloud schedules on the server, so case
# 18 is rewritten: the owner's Apple ID (their Gmail address, the only one iCloud
# organizes as) organizes and the owner's Microsoft account attends; the run then accepts
# from Graph, waits for the answer in iCloud and looks for it in Gmail (prediction 4).
# The calendar nitya-spike is made for the run and deleted at its end, its events first,
# which cancels the meeting for the Microsoft account.
# Needs the owner's iCloud app-specific password and Google tokens, decrypted by agenix
# into $NITYA_SECRETS (default ~/.config/nitya). From the repo root:
#   nix develop .#calendar-spike -c spikes/calendar-roundtrip/run-icloud.sh
set -euo pipefail
here=spikes/calendar-roundtrip
out=${YAJNA_RUN_DIR:-$here/out/icloud}
rm -rf "${out:?}" && mkdir -p "$out"
cargo build -q --locked --manifest-path "$here/Cargo.toml"
python3 -I "$here/providers.py" icloud "$here/target/debug/calendar-roundtrip" "$here/corpus" "$out"
