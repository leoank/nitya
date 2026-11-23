---
id: PHS-20261123-bhenjn
type: phase
schema_version: 1
title: "Phase 0: spikes and design"
project: PRJ-0001
status: planned           # planned | active | done | abandoned
since: null               # the commit it started at: yajna phase start
until: null               # the commit it finished at: yajna phase done
created: 2026-11-23
author: agent:claude
tags: []
---

## Goal
Test what the design rests on before building it, and settle the repo. Done when the four
experiments have outcomes, the decisions they touch are confirmed or superseded, and the
repo has a flake with a dev shell and an empty workspace that builds.

## Progress
One item per piece of work, ticked when it lands; add items as the plan grows and keep the
done ones. The project page's *Progress* lists this phase on one line.

- [x] 1. The architecture and decisions ADR-0001 to ADR-0007 from the owner's interview ([[AGS-20261123-bhdnxc]])
- [ ] 2. Calendar round-trip fidelity across Google, Graph and CalDAV ([[EXP-20261123-bhfzfs]]), first: the owner's chosen first risk
- [ ] 3. The key hierarchy and the sync delegate ([[EXP-20261123-bhfzeh]])
- [ ] 4. OAuth refresh tokens on a headless server for 30 days ([[EXP-20261123-bhfz49]]), started early since it takes a month
- [ ] 5. nitya to saarthi: spawn, plan, approval from the phone ([[EXP-20261123-bhfz7e]])
- [ ] 6. The flake: dev shell (Rust, protoc), Cargo workspace with `server` and `node` crates, `nix flake check`
- [ ] 7. Decide how poll respondents reach nitya (open question on [[PRJ-0001]]) before phase 4

## Notes
