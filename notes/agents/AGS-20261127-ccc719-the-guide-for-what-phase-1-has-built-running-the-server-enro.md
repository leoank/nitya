---
id: AGS-20261127-ccc719
type: agent_session
schema_version: 1
title: "the guide for what phase 1 has built: running the server, enrolling, keys, policies, the audit log"
model: "claude-opus-5-5"
harness: "claude-code"
started: 2026-11-27T11:16+00:00
finished: 2026-11-27T11:20+00:00
created_entities: []
edited_entities: []
outcome: done             # done | failed | aborted | pending
created: 2026-11-27
author: agent:claude
---

## Task

"Make sure you are also updating the docs and guides for the features you have already
built, and keep doing this for the new features you'll build." The guide (mdBook,
`docs/guide`) still described phase 0 after phase 1 items 1 to 6 had landed.

## What happened

- `nitya-server --help` and `nitya-node --help`; `docs/guide/build.py` generates their
  reference pages from them, as it does for `nitya-consent`, so the reference cannot drift
  from the code. The docs build now builds both binaries.
- A *Using nitya* section: running the server and enrolling devices, your keys, who may
  do what, the audit log, each with the commands as they behave on the tailnet.
- *Where it stands*, the introduction's caveat, *How it works* (device identity, keys,
  policies, the log) and the deployment overview brought up to date.
- `nix build .#docs` passes, every relative link resolves.
- The instruction to keep the guide current with each feature is in this agent's memory
  and applies to item 7 next.
