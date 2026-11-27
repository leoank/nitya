---
id: AGS-20261127-hy864v
type: agent_session
schema_version: 1
title: "phase 1 item 7: nitya deployed on ghost001, rogue's node enrolled, and the first-dial race it showed"
model: "claude-opus-5-5"
harness: "claude-code"
started: 2026-11-27T16:20+00:00
finished: 2026-11-27T16:25+00:00
created_entities: []
edited_entities: [PHS-20261123-bhen36]
outcome: done             # done | failed | aborted | pending
created: 2026-11-27
author: agent:claude
---

## Task

Finish phase 1 item 7 once the owner had deployed neusis: ghost001 runs the server, rogue
has the node; enroll, make the account's keys, check it all. The owner reported that each
command first failed with "lookup nitya: no such host" and worked when run again.

## What happened

- The owner made the delegate key with a script from this session (`age-keygen` into a
  private temporary directory, `agenix edit -i`, `agenix rekey`), committed and pushed
  neusis; ghost001 deployed, rogue switched.
- Two launchd agents on rogue started right after the switch, before agenix had placed
  their secrets: nitya's hourly check (fine on its next run) and sandhi's daemon (kept the
  failure until the owner restarted it).
- The owner enrolled rogue's node and ran `keys init` here. Diagnosis of the first-try
  failures: each `nitya-node` run starts tsnet from saved state, which reports Running
  before the tailnet's peers arrive, so the first dial cannot resolve `nitya`; the
  end-to-end checks never saw it because their nodes joined fresh. Three `whoami` runs:
  two failed within a second.
- Fix: the tailnet crate's `dial` retries a failed lookup for up to 15 s. Six runs of the
  fixed build all pass, some after waiting up to 6 s.
- Checked: `keys status` (rogue holds the keys of ank, user key version 1), `audit verify`
  (to entry 9), and on ghost001 over sandhi, read-only, `nitya-server` active as user
  `nitya` with no restarts, on the tailnet as `nitya`, the enrollment and the first key
  set in its log. Reading /var/lib/nitya needs the owner's sudo password; not looked at.
- Item 7 ticked; the guide's status page says phase 1 is at its end.
