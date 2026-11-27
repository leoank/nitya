---
id: AGS-20261127-1x4w3q
type: agent_session
schema_version: 1
title: "phase 1 item 5: Cedar policies and schema for users, devices and agents"
model: "claude-opus-5-5"
harness: "claude-code"
started: 2026-11-27T01:44+00:00
finished: 2026-11-27T01:59+00:00
created_entities: []
edited_entities: [PHS-20261123-bhen36]
outcome: done             # done | failed | aborted | pending
created: 2026-11-27
author: agent:claude
---

## Task

"Commit and move to next item": phase 1 item 5, Cedar policies and schema for users,
devices and agents, validated in CI (ADR-0006).

## What happened

- Asked the owner two questions; chosen: nodes act as the user like the app, and each
  agent grant is a linked template with its own policy id. Recorded as ADR-0014.
- Decided without asking, for the owner to check in ADR-0014: the entity types and
  their nesting under `User`; a `writes` action group used only to require a plan;
  `draft.create` counted as a write; principals identified by device key; the server
  refusing to start on policies that do not validate.
- `crates/server/policies/`: the schema, five policies and nine grant templates;
  `policy.rs` loads, validates strictly and decides, returning the deciding policy ids.
  6 tests: validation, devices limited to their account, grants not implying each
  other and covering a source's items, writes needing a plan, agents never holding keys
  or approving plans, unknown grant actions refused.
- The `Keys` service asks Cedar for `keys.read` and `keys.write`. On the leoank tailnet
  an agent certificate is refused with `agents-never-touch-keys`; a node gets its keys.
- Surprise: `nix develop` hung on the notebook commands (another session's `nix develop`
  was running at the same time); used the same yajna binary directly. An interrupted
  command still wrote a start line under item 3's session id; a correction line follows
  it in the log.
