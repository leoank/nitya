---
id: AGS-20261126-f96pd2
type: agent_session
schema_version: 1
title: "Start EXP-20261123-bhfz49: a 30-day token probe on ghost001 (nitya module, neusis wiring)"
model: "claude-opus-5-5"
harness: "claude-code"
started: 2026-11-26T13:54+00:00
finished: 2026-11-26T14:03+00:00
created_entities: [RUN-20261126-f98mes]
edited_entities: [EXP-20261123-bhfz49]
outcome: done             # done | failed | aborted | pending
created: 2026-11-26
author: agent:claude
---

## Task

Phase 0 item 4: start EXP-20261123-bhfz49, the 30-day token test on ghost001. The owner
chose to start it before item 3 because its clock is the long pole.

## What happened

- `nitya-token-probe` (stdlib Python) with an offline test against a fake that rotates
  Microsoft tokens; `nixosModules.token-probe`, a 6-hourly oneshot as a dynamic user with
  the tokens as systemd credentials; a module check in `nix flake check`.
- One real check on rogue: Google, both Microsoft chains and iCloud worked; Microsoft
  rotated its token.
- neusis: `features.nixos.nitya-token-probe` (declaring the nitya input with its neusis
  following neusis itself), its test, two imports in ghost001.nix. ghost001 evaluates
  and its 13 machine tests pass against the local nitya. The owner chose to commit only
  nitya; the neusis files are left uncommitted in their checkout (flake.nix regenerated,
  no lock change yet, since the input must lock to pushed nitya).
- Surprise: my staged-tree check piped through `tail`, which hid a failing notebook check;
  commit a1c192f (pushed) fails validate alone. The check now stops on failure.
