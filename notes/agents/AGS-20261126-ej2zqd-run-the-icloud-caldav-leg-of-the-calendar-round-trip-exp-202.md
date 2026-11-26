---
id: AGS-20261126-ej2zqd
type: agent_session
schema_version: 1
title: Run the iCloud CalDAV leg of the calendar round-trip (EXP-20261123-bhfzfs) on a throwaway calendar
model: "claude-opus-5-5"
harness: "claude-code"
started: 2026-11-26T13:15+00:00
finished: 2026-11-26T13:36+00:00
created_entities: [RUN-20261126-ej2zgp]
edited_entities: [EXP-20261123-bhfzfs, PRJ-0001, PHS-20261123-bhenjn]
outcome: done             # done | failed | aborted | pending
created: 2026-11-26
author: agent:claude
---

## Task

Run the iCloud CalDAV leg of EXP-20261123-bhfzfs with the owner's app-specific password;
the owner chose real invitations for case 18, first to their Google account, then (when
that proved to be the Apple ID itself) to their Outlook.com account.

## What happened

- `ICloud` driver in providers.py: principal and calendar-home discovery, MKCALENDAR of a
  throwaway calendar, one resource per UID with If-None-Match and If-Match, deletion.
- RUN-20261126-ej2zgp: 23/24 meaning kept on import, 24/24 on write-back, UIDs kept;
  the most faithful provider. Case 18 only differs in the organizer's form.
- Surprises: the owner's Apple ID is their Gmail address, the only address iCloud
  organizes as, so the first full run invited itself and sent nothing; iCloud then
  marked an invitation to Outlook.com delivered (1.2) that never arrived. The first run
  wrote the iCloud principal path (an account identifier) into out/ unredacted; caught
  before any commit, redaction extended, outputs overwritten.
- I printed the subjects of two unrelated inbox messages while looking for the
  invitation; only in the session, nowhere written.
- With all four legs run, drafted the experiment's Conclusion (outcome positive, review
  pending), updated the project page's line and ticked phase 0 item 2.
- Not committed: the owner has not asked. The run needs `yajna run record` with the
  commit that holds the driver.
