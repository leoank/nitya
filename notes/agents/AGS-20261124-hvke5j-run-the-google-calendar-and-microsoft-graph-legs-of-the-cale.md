---
id: AGS-20261124-hvke5j
type: agent_session
schema_version: 1
title: Run the Google Calendar and Microsoft Graph legs of the calendar round-trip (EXP-20261123-bhfzfs) on throwaway calendars
model: "claude-opus-5-5"
harness: "claude-code"
started: 2026-11-24T16:15+00:00
finished: 2026-11-24T16:56+00:00
created_entities: [RUN-20261124-hwkxry, RUN-20261124-hwkyzz]
edited_entities: [EXP-20261123-bhfzfs]
outcome: done             # done | failed | aborted | pending
created: 2026-11-24
author: agent:claude
---

## Task

Check that nitya-consent's tokens work against the real providers, then run the Google
Calendar and Microsoft Graph legs of EXP-20261123-bhfzfs on throwaway calendars. The owner
chose a calendar made and deleted by each run, and, for the attendee case on Graph, real
invitations between their own Microsoft and Google accounts.

## What happened

- Both refresh tokens refresh and read calendars and mail. Microsoft returns a new
  refresh token on each refresh: the server must keep the latest in its own store.
- `providers.py` (stdlib Python, spike only) maps iCalendar to each API's JSON and back;
  the Rust harness got `edit` and `compare` subcommands; `.#calendar-spike` got Python.
- RUN-20261124-hwkxry (Google): 21/24 meaning kept on import, 23/23 on write-back, 1
  refused. RUN-20261124-hwkyzz (Graph): 18/24, 22/22, 2 refused, UIDs replaced; the
  acceptance from Google reached the Outlook.com inbox in seconds but Graph's attendee
  status never changed in 10 minutes (two runs).
- Surprises: Google imports only events the calendar itself organizes or attends, and a
  secondary calendar's owner is its id. My driver had bugs the runs caught (an EXDATE
  deleted twice, redaction after folding, an encoded id written back); fixed and rerun,
  outputs overwritten. The repo is public: owner addresses, names and calendar ids are
  replaced with placeholders in out/, checked by grep.
- Not committed: the owner has not asked. Both runs need `yajna run record` with the
  commit that holds `providers.py` and the scripts; `yajna validate` reports that until
  then.
