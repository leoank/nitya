---
id: RUN-20261124-hwkxry
type: run
schema_version: 1
kind: compute               # bench | compute | assembly | calibration
project: PRJ-0001
experiment: EXP-20261123-bhfzfs
protocols: []
status: done              # planned | in_progress | done | aborted
created: 2026-11-24
finished: 2026-11-24T16:42+00:00
host: rogue
author: agent:claude
script: spikes/calendar-roundtrip/run-google.sh           # executable that re-creates this run, relative to the repo root
commit: 668cdf66d42215259c461a1259ba3a94e571dc43
outputs:
  - path: spikes/calendar-roundtrip/out/google/table.tsv
    sha256: 13540a572e2ae2417eeadc2741d1d43cfdc93d657f058fb48ed7c46cd2a1dfb7
  - path: spikes/calendar-roundtrip/out/google/details.md
    sha256: 2ebce4f02d06af86ab4978a1c8772b190f5a0af5d3e4603870a245f3e7bd1801
---

## Result

| Key | Value |
|-----|-------|
| cases | 24 |
| import_same | 21 |
| writeback_same | 23 |
| refused | 1 |
| uid_kept | 23 |

## Notes

Every corpus case imported into a throwaway calendar `nitya-spike` (zone Europe/London)
on the owner's Google account with `events.import` (no mail is sent), overrides imported
with the series' iCalUID and `originalStartTime`; read back with `events.list?iCalUID=`
and mapped to iCalendar by `providers.py`; then edited as in the Radicale run and written
back with `events.update` (`sendUpdates=none`), read again and compared. The calendar is
deleted at the end. `out/google/table.tsv` and `details.md` hold the rows.

What it showed:
- Meaning kept on import for 21 of 24 cases and on write-back for every case written (23).
  Google keeps the recurrence lines as given: RRULE (only its parts reordered), EXDATE
  and RDATE (case 16), BYSETPOS (15), and a cancelled instance as a cancelled exception
  with its data (09). Moved instances (08), an all-day series with one timed instance
  (22), DST crossings (11, 12), +05:30 (13) and long Unicode text (19) are unchanged.
- The UID is kept (`uid_kept` 23 of 23 written).
- Changed: a floating time (04) becomes a time in the calendar's zone; Google has none.
- Refused before writing: RANGE=THISANDFUTURE (10a); Google has no range, a driver splits
  the series as 10b shows (10b keeps meaning, its RELATED-TO is lost).
- Attendees (18): Google imports an event only when the calendar's owner organizes or
  attends it (`participantIsNeitherOrganizerNorAttendee`), and a secondary calendar's
  owner is the calendar's id, not the account: the account's address was refused too. With
  the corpus owner as the calendar, it imports: PARTSTATs kept; ROLE CHAIR and
  NON-PARTICIPANT become REQ-PARTICIPANT, RSVP is dropped, and the room becomes LOCATION
  "Room" as well as an attendee.
- URL (as `source`), ATTACH by URL (`attachments`), both alarms (`reminders.overrides`),
  CLASS and TRANSP map to fields. CATEGORIES, PRIORITY and X- properties, for which Google
  has no field, round-trip as private extended properties, a choice of the driver.

Not controlled by the script: Google's service as of the run; the owner's account and
tokens (`NITYA_SECRETS`, default ~/.config/nitya). Earlier attempts the same day showed
the import refusals above and four driver bugs (redaction across folded lines, an
encoded calendar id written back as an attendee); their outputs were overwritten.


## Reproduce
Run `script` from the repo root in the project's dev shell at `commit`; it must produce the
same outputs. Note here what it cannot control.

## Checklist
- [x] script ran from the repo root in `.#calendar-spike`
- [x] `yajna run record` with the commit that holds the script 
