---
id: AGS-20261123-bhdnxc
type: agent_session
schema_version: 1
title: "Bootstrap the nitya project: interview, notebook, design and first experiments"
model: claude-opus-5-5
harness: claude-code
started: 2026-11-23T10:30+00:00
finished: 2026-11-23T10:35+00:00
created_entities: [PRJ-0001, PHS-20261123-bhenjn, PHS-20261123-bhen36, PHS-20261123-bhen6w, PHS-20261123-bhend2, PHS-20261123-bhensm, PHS-20261123-bheny1, PHS-20261123-bhenge, PHS-20261123-bhepkn, EXP-20261123-bhfzfs, EXP-20261123-bhfzeh, EXP-20261123-bhfz49, EXP-20261123-bhfz7e]
edited_entities: []
outcome: done
created: 2026-11-23
author: agent:claude
---

## Task
Bootstrap a new project from `/yajna:project` with no brief, in the empty directory
`~/workspace/repos/leoank/nitya`: interview the owner, then create the notebook, the
project page, phases, design and first experiments.

## What happened
- No `yajna.toml` and no brief: asked where the notebook should live (a project notebook
  in nitya) and what nitya is (software); followed the code-project shape inside
  `/yajna:project`.
- Read saarthi's `origin/main` (the local `saarthi/main` worktree is months behind and was
  left untouched): its tailnet design, the service relay (ADR-0052), the Nitya tab
  reserved in its app (ADR-0068). Read sandhi's notebook and yajna's Google sync
  experiments and ADR-0011.
- Two background surveys of prior art: calendar, polls and email (cal.diy, msgvault, Rallly,
  Twenty, Stalwart, Radicale, himalaya, Inbox Zero, Zero) and Wire's core-crypto plus
  open-source authorization (Cedar, casbin-rs, OpenFGA, SpiceDB, Permify, Keto, Biscuit).
  Condensed into ten reading notes under `notes/references/`.
- Interview, in six rounds: the goal sentence agreed verbatim; one user, multi-account
  design; phase 1 scope; the owner reversed an earlier answer and chose nitya's own
  server and nodes on tailnets over saarthi's relay; own store; msgvault's agent model;
  Rust; node roles; all four crypto properties; tasks only; ghost001; first risk calendar
  fidelity. Read back a summary; the owner said yes and accepted the proposed crypto and
  policy decisions (Cedar, key hierarchy, device certificates, no MLS).
- `git init` and `yajna init project`; ADR-0001 to ADR-0007 and `docs/architecture.md`;
  eight phases with goals and checklists; four experiments; `[repos]` entries for yajna
  and saarthi in `yajna.toml`.
- Decided without asking: phase checklists' item wording; experiment predictions about
  Google and Microsoft token lifetimes, marked as believed and to be checked; the
  reading notes' status `skimmed`.
- `yajna init project` did not scaffold a flake (only `yajna new project --code` does,
  and the project page already existed), so the flake is item 6 of phase 0, not done.
- `yajna ref --note` wrote bib titles containing `: ` unquoted into YAML frontmatter,
  which failed validation; quoted them by hand. A yajna bug worth fixing there.
- `yajna validate` passes. Nothing committed.
