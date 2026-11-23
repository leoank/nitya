---
id: PHS-20261123-bhensm
type: phase
schema_version: 1
title: "Phase 4: meeting polls"
project: PRJ-0001
status: planned           # planned | active | done | abandoned
since: null               # the commit it started at: yajna phase start
until: null               # the commit it finished at: yajna phase done
created: 2026-11-23
author: agent:claude
tags: []
---

## Goal
Meeting polls outside people can answer. Done when a poll sent to people without accounts is
answered, the chosen slot is booked on the owner's calendar and invitations go out, with
nothing else of nitya reachable from outside.

## Progress
One item per piece of work, ticked when it lands; add items as the plan grows and keep the
done ones. The project page's *Progress* lists this phase on one line.

- [x] 1. Accept or replace ADR-0008 (proposed: a Cloudflare Worker with D1 holding only poll data) for the respondents' public path: accepted 2026-11-23
- [ ] 2. Polls with capability tokens per invitee and per response, rotation and revocation (@rallly)
- [ ] 3. Slots from free/busy; finalising books the event and sends invitations once (@rfc5546-itip)
- [ ] 4. The poll Worker and D1 schema, deployed with `wrangler` from the dev shell; nitya's signed push and pull (ADR-0008)

## Notes
