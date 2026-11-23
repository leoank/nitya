---
id: EXP-20261123-bhfz7e
type: experiment
schema_version: 1
title: Can nitya, as a saarthi control-service client, spawn a session holding a scoped nitya grant, receive a write plan, and execute it only after approval from the phone, with every step audited?
project: PRJ-0001
status: planned           # planned | active | paused | concluded | abandoned
outcome: null             # null | positive | negative | inconclusive | infra
influenced_by: []
created: 2026-11-23
author: agent:claude
protocols: []
materials: []
tags: []
review: pending
---

## Question
Can nitya hand a job to an agent through saarthi end to end: spawn a session on a node
through saarthi's control service holding a narrowed nitya grant, receive a write plan
from it, have the owner approve it from the phone, and execute it, with every step in the
audit log? And is approving fast enough that the owner will use it (@inbox-zero)?

## Hypothesis and predictions
H1: saarthi's spawner (saarthi ADR-0034) and a nitya grant given to the session as an
environment credential are enough; no change to saarthi is needed for the first version.

1. A spawned session reads the owner's calendar through nitya's API with a
   `calendar.read` grant and proposes a plan (e.g. move a meeting); a write without an
   approved plan is refused by policy.
2. The plan reaches the phone (a notification, later the Nitya workspace) and approval is
   one tap; from proposal to execution under a minute when the owner is at the phone.
3. The audit log holds the spawn, the grant, each read, the plan, the approval and the
   write, under one operation id.

If 1 needs a saarthi change (e.g. passing a credential to a spawned session), that becomes
an item on saarthi's phase 4 rather than a nitya workaround.

## Plan
1. A stub nitya server (no real sync; a fake calendar) with grants, a plan endpoint and
   the audit log from [[EXP-20261123-bhfzeh]]'s spike.
2. Register nitya as a saarthi control client with its own token; spawn a session on a
   node with a prompt and the grant.
3. Approve from the phone (an ntfy action at first) and execute.
4. Repeat ten times; time each step; read the log back.

## Success criteria
- Ten jobs from spawn to executed write, each fully in the log, none executed without
  approval.
- Median time from proposal to execution under a minute with the owner at the phone.
- A list of what saarthi lacks, filed on saarthi's notebook if anything.

## Runs
One line per run after its link, what was tried and what it showed; `yajna validate`
rejects a bare link here. Metric runs also land in results.tsv.

## Conclusion
Written once, by a human, when status becomes `concluded`.
