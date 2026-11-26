---
id: AGS-20261126-fexqja
type: agent_session
schema_version: 1
title: "Phase 0 item 3: the key-hierarchy spike (EXP-20261123-bhfzeh)"
model: "claude-opus-5-5"
harness: "claude-code"
started: 2026-11-26T14:04+00:00
finished: 2026-11-26T14:09+00:00
created_entities: [RUN-20261126-fexqm6]
edited_entities: [EXP-20261123-bhfzeh, PRJ-0001, PHS-20261123-bhenjn]
outcome: done             # done | failed | aborted | pending
created: 2026-11-26
author: agent:claude
---

## Task

Phase 0 item 3: the key-hierarchy spike of EXP-20261123-bhfzeh, after starting item 4.

## What happened

- `spikes/key-hierarchy`: ADR-0005's hierarchy with age, XChaCha20-Poly1305, Argon2id and
  an Ed25519-signed hash chain; six scenario tests and a timing binary.
- Predicted before running, from reading ADR-0005: re-wrapping unchanged DEKs does not
  revoke. The run confirmed it; rotating the DEKs and the append key fixes it.
- RUN-20261126-fexqm6 on rogue: 6/6 tests, ~6 µs an item, ~70 ms a revocation.
- Drafted the conclusion (positive, review pending) and ADR-0010 (proposed); updated the
  project page's lines for this experiment and EXP-20261123-bhfz49, ticked phase 0 item 3.
- Not committed: awaiting the owner. The run needs `yajna run record` with the commit
  that holds the spike.
