---
id: PHS-20261123-bhen6w
type: phase
schema_version: 1
title: "Phase 2: calendar sync and tasks"
project: PRJ-0001
status: active
since: 917bb147d7ecfc3a3841d9f27e5132e907e27be9
until: null               # the commit it finished at: yajna phase done
created: 2026-11-23
author: agent:claude
tags: []
---

## Goal
The owner's calendars live in nitya, two-way, and tasks exist beside them. Done when the
owner's Google, Microsoft 365 and CalDAV calendars are synced both ways for two weeks with
no lost or duplicated event, and tasks can be made from an event or an email.

## Progress
One item per piece of work, ticked when it lands; add items as the plan grows and keep the
done ones. The project page's *Progress* lists this phase on one line.

- [x] 1. Channel engine: drivers, opaque cursors, sync stages, stale-job recovery, polling (ADR-0003, @twenty-crm); ADR-0017: only with consent to `tokens` and the class, every minute, `nitya-node sync now`; no driver yet
- [ ] 2. Google Calendar driver, two-way, with the contract from [[EXP-20261123-bhfzfs]]
- [ ] 3. Microsoft Graph calendar driver, two-way
- [ ] 4. CalDAV driver, two-way
- [ ] 5. Free/busy across all calendars (needed by polls)
- [ ] 6. Tasks: create, due dates, reminders, linked to an event or a message
- [ ] 7. Desktop notifications from the node for events and tasks

## Notes
