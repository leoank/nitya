#!/usr/bin/env bash
# Re-creates RUN-20261127-mr3rsk for EXP-20261127-mqzh92: Google Calendar's sync tokens,
# a garbled token and If-Match, on a throwaway calendar of the owner's account. From the
# repo root:
#   nix develop .#calendar-spike -c spikes/google-sync/run.sh
# Needs the owner's Google client JSON and nitya-consent token JSON, as agenix decrypts
# them into $NITYA_SECRETS (default ~/.config/nitya). Nothing secret is printed or written.
set -euo pipefail
secrets=${NITYA_SECRETS:-$HOME/.config/nitya}
out="${YAJNA_RUN_DIR:-$(dirname "$0")/out}"
mkdir -p "$out"
python3 -I "$(dirname "$0")/probe.py" "$secrets/google_client.json" "$secrets/google_personal_token.json" "$out"
