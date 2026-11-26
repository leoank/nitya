#!/usr/bin/env bash
# RUN-20261126-f98mes for EXP-20261123-bhfz49: collect the token probe's log from the
# host it runs on (nixosModules.token-probe, every 6 hours) and summarise it. The probe
# itself runs as a systemd timer there; this only reads what it wrote, and can be run
# any number of times during and after the 30 days. From the repo root:
#   nix develop -c spikes/token-probe/collect.sh            # PROBE_HOST defaults to ghost001
set -euo pipefail
here=spikes/token-probe
out=${YAJNA_RUN_DIR:-$here/out}
host=${PROBE_HOST:-ghost001}
mkdir -p "$out"
# the state directory belongs to the service's dynamic user: read it as root
ssh -o BatchMode=yes "$host" sudo cat /var/lib/nitya-token-probe/probe.jsonl > "$out/probe.jsonl"
nix run .#nitya-token-probe -- report "$out/probe.jsonl" | tee "$out/report.txt"
