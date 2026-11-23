---
id: PHS-20261123-bheny1
type: phase
schema_version: 1
title: "Phase 5: agents: nitya spawns saarthi sessions, phone approvals, then MCP through the node"
project: PRJ-0001
status: planned           # planned | active | done | abandoned
since: null               # the commit it started at: yajna phase start
until: null               # the commit it finished at: yajna phase done
created: 2026-11-23
author: agent:claude
tags: []
---

## Goal
Agents do the mundane work under the owner's grants. Done when nitya runs a triage and a
scheduling job through saarthi with plans approved from the phone, and a session started by
hand can use nitya through the node's MCP server under that machine's grants (ADR-0004).

## Progress
One item per piece of work, ticked when it lands; add items as the plan grows and keep the
done ones. The project page's *Progress* lists this phase on one line.

- [ ] 1. Grants and plans in the server; agent writes forbidden without an approved plan
- [ ] 2. nitya as a saarthi control client spawning job sessions with narrowed grants ([[EXP-20261123-bhfz7e]])
- [ ] 3. Approval from the phone, one tap and batched
- [ ] 4. A triage job and a scheduling job
- [ ] 5. MCP server in the node

## Notes
