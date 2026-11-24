---
id: RUN-20261124-hwkyzz
type: run
schema_version: 1
kind: compute               # bench | compute | assembly | calibration
project: PRJ-0001
experiment: EXP-20261123-bhfzfs
protocols: []
status: done              # planned | in_progress | done | aborted
created: 2026-11-24
finished: 2026-11-24T16:55+00:00
host: rogue
author: agent:claude
script: spikes/calendar-roundtrip/run-graph.sh           # executable that re-creates this run, relative to the repo root
commit: 668cdf66d42215259c461a1259ba3a94e571dc43
outputs:
  - path: spikes/calendar-roundtrip/out/graph/table.tsv
    sha256: 8cd2f8f251a189613b2230df3cacbd755ac9885ab8b3f139560d11a4016c6538
  - path: spikes/calendar-roundtrip/out/graph/details.md
    sha256: 2567ca959f13f2482d09f7d48332c694e705a6f92d0fae09b3d6a9019edd065a
---

## Result

| Key | Value |
|-----|-------|
| cases | 24 |
| import_same | 18 |
| writeback_same | 22 |
| refused | 2 |
| uid_kept | 0 |
| invite_reached_google | yes, before the run looked (within minutes) |
| reply_reached_graph_s | null (not within 600 s) |

## Notes

Every corpus case written to a throwaway calendar `nitya-spike` on the owner's Outlook.com
account through Graph v1.0: an RRULE mapped to `patternedRecurrence`, EXDATEs and
cancelled instances deleted by instance, overrides PATCHed on the instance at their
`originalStart`; read back with `Prefer: outlook.timezone="UTC"`, EXDATEs recovered as
the rule's instances that have none. Then edited as in the Radicale run and written back
(a series' recurrence, start and end are left out of the PATCH when unchanged, since
patching them resets its exceptions). Case 18 was rewritten so the owner's Microsoft
account organizes and the owner's Google account attends (the owner chose this; Graph
cannot save a meeting without inviting). After all cases the run accepted the invitation
from Google (`sendUpdates=all`) and polled Graph for the answer. Events, then the
calendar, are deleted at the end. A first full run at 16:25 gave the same table.

What it showed:
- Meaning kept on import for 18 of 24 and on write-back for 22 of 22 written. Kept: zones
  including DST crossings, +05:30 and New York in a London calendar, all-day and
  multi-day events, COUNT and UNTIL, BYDAY=-1FR (14) and BYSETPOS=-1 over weekdays (15,
  as Graph's relativeMonthly over several days), a moved instance (08), an all-day series
  with one timed instance (22), long Unicode text (19), CATEGORIES (20), CLASS (23).
- Graph replaces the UID with its own iCalUId (`uid_kept` 0): nitya keeps the mapping.
  Graph re-expresses the RRULE (BYDAY always listed, UNTIL as a date), same instances.
- Changed: floating time (04) becomes zoned; a cancelled instance (09) becomes a missing
  one (an EXDATE): Graph has no cancelled-but-kept instance; URL, ATTACH by URL and X-
  properties (20) have no field; of two alarms (21) one is kept.
- Refused before writing: RANGE=THISANDFUTURE (10a) and RDATE (16).
- Attendees (18): kept apart from RSVP. The invitation and the update reached Google
  within seconds; the acceptance from Google reached the Outlook.com inbox about 20 s
  after it was sent (16:44:50; the first run 16:26:50), but Graph's attendee status
  stayed `none` for the 10 minutes the run waited. Outlook.com did not apply the
  iTIP REPLY to the event by itself. Prediction 4 holds for delivery, not for the status.

Not controlled by the script: Graph and Outlook.com as of the run; mail delivery times;
the owner's accounts and tokens (`NITYA_SECRETS`). The run sends real mail between the
owner's two accounts (an invitation, an update, a cancellation).


## Reproduce
Run `script` from the repo root in the project's dev shell at `commit`; it must produce the
same outputs. Note here what it cannot control.

## Checklist
- [x] script ran from the repo root in `.#calendar-spike`
- [x] `yajna run record` with the commit that holds the script 
