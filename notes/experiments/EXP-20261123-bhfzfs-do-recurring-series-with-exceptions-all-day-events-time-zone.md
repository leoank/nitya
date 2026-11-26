---
id: EXP-20261123-bhfzfs
type: experiment
schema_version: 1
title: Do recurring series with exceptions, all-day events, time zones and attendees round-trip between Google Calendar, Microsoft Graph, CalDAV and a store of raw iCalendar with provider ids, without loss?
project: PRJ-0001
status: concluded         # planned | active | paused | concluded | abandoned
outcome: positive         # null | positive | negative | inconclusive | infra
influenced_by: []
created: 2026-11-23
author: agent:claude
protocols: []
materials: []
tags: []
review: pending
---

## Question
Can nitya keep raw iCalendar plus provider ids as its record (ADR-0003) and still write
events back to Google Calendar, Microsoft Graph and a CalDAV server, and read them again,
without losing or changing what matters: recurring series with exceptions, all-day
events, time zones, attendees and their replies? Everything in phase 2 rests on this, and
the owner chose it as the first risk to test.

## Hypothesis and predictions
H1: each provider's event can be mapped to and from iCalendar (@rfc5545-icalendar) so that
a cycle of import, edit in nitya, write back and re-import leaves no semantic difference.

1. Google Calendar (@gcal-sync): simple events, all-day events, a weekly series with one
   moved and one cancelled instance, and a series in a time zone other than the
   calendar's survive. If false we expect overrides to come back as separate events or
   `RECURRENCE-ID`s in the wrong zone.
2. Microsoft Graph (@msgraph-delta): the same corpus survives except where Graph's
   recurrence model cannot express an RRULE (e.g. BYSETPOS combinations, RDATE); those are
   rejected cleanly at write time, never silently changed.
3. CalDAV (@rfc4791-caldav, Radicale as reference server and one real provider such as
   iCloud): byte-level round trip of the stored `.ics` apart from `DTSTAMP`, `SEQUENCE`
   and provider extensions.
4. Attendee replies (iTIP, @rfc5546-itip) are reflected in `PARTSTAT` after a provider
   delivers them, without nitya sending duplicate invitations (Google's `sendUpdates`).

If H1 fails for a provider, nitya keeps that provider's own representation as the record
for its channel and ADR-0003 is narrowed.

## Plan
1. Write a corpus of about 20 hard cases as `.ics` files: series with EXDATE, moved and
   cancelled instances, this-and-following splits, all-day and multi-day events, floating
   times, DST crossings, events in a non-calendar zone, attendees with each PARTSTAT,
   long descriptions, attachments by URL.
2. Scratch calendars on a personal Google account, a Microsoft 365 or Outlook.com account,
   a local Radicale (@radicale) and one hosted CalDAV account.
3. A throwaway Rust spike (in `spikes/`) with a candidate iCalendar crate and each
   provider's client: create each case in the provider, import, compare with the source;
   edit in the store, write back, re-import, compare.
4. A semantic diff: expand recurrences over two years into instances (start, end, zone,
   summary, attendees, status) and compare instance sets, plus the raw properties kept.
5. One run per provider; record the corpus, the crate chosen and the lossy fields.

## Success criteria
- Each provider: every case either round-trips with an empty semantic diff or is refused
  at write time with a reason; nothing is silently changed.
- A table of fields each provider loses or rewrites, which becomes the channel drivers'
  contract.
- An iCalendar crate chosen (or a reason to write the parts we need).
- Negative for a provider if overrides or zones are changed silently and cannot be
  detected before writing.

## Runs
One line per run after its link, what was tried and what it showed; `yajna validate`
rejects a bare link here. Metric runs also land in results.tsv.
- [[RUN-20261123-f5xbpv]] crates: calcard and icalendar's Calendar keep the meaning of all 24
  cases, but icalendar writes invalid, random UIDs into time zones and alarms and its
  low-level parser drops TEXT escapes; calcard only reorders RRULE parts
- [[RUN-20261123-f5xbz0]] local Radicale: all 24 keep meaning and every property on import
  and write-back, one resource per UID; a two-UID resource is refused; bytes are
  re-serialized, so change detection must use ETags
- [[RUN-20261124-hwkxry]] Google Calendar API: meaning kept for 21 of 24 on import and all
  23 written back; RRULE, EXDATE, RDATE and the UID as given; floating time becomes zoned,
  THISANDFUTURE refused; imports only events the calendar itself organizes or attends
- [[RUN-20261124-hwkyzz]] Microsoft Graph: meaning kept for 18 of 24 on import and all 22
  written back; own UIDs; RDATE and THISANDFUTURE refused; a cancelled instance becomes
  an EXDATE, URL, attachments and X- properties and a second alarm are lost; an iTIP
  reply reached the inbox in seconds but not the event's attendee status
- [[RUN-20261126-ej2zgp]] iCloud CalDAV: meaning kept for 23 of 24 on import and all 24
  written back, UIDs kept; keeps floating time, RDATE, THISANDFUTURE, X- properties and
  both alarms; rewrites VTIMEZONEs and the organizer's form; its invitation to the
  owner's Outlook.com account was marked delivered but never arrived
- [[RUN-20261124-hwkxry]] compute run, not yet described
- [[RUN-20261124-hwkyzz]] compute run, not yet described
- [[RUN-20261126-ej2zgp]] compute run, not yet described

## Conclusion
*Draft by agent:claude for the owner's review.*

H1 holds in the form ADR-0003 needs: nitya can keep raw iCalendar with provider ids as its
record. No provider changed an override or a zone (the negative criterion): moved
instances, exceptions, DST crossings, a +05:30 zone, a New York series in a London
calendar and an all-day series with a timed instance kept their instances everywhere they
were written. What a provider cannot hold is known before writing, from the driver's
mapping, so it is refused or kept in nitya's store, never silently lost from the record.

Each provider's contract, from the runs:
- **CalDAV** (Radicale [[RUN-20261123-f5xbz0]], iCloud [[RUN-20261126-ej2zgp]]): a
  faithful store of iCalendar, every case and UID kept, including floating time, RDATE,
  THISANDFUTURE and X- properties. Servers re-serialize (RRULE part order, iCloud its own
  VTIMEZONEs, the organizer as a principal urn with an EMAIL parameter), so change
  detection uses ETags and sync tokens, and comparisons are semantic, never by bytes.
  One resource per UID.
- **Google Calendar API** ([[RUN-20261124-hwkxry]]): keeps RRULE, EXDATE and RDATE lines and
  the UID as given. No floating time (becomes the calendar's zone); no
  RANGE=THISANDFUTURE (the driver splits the series, as 10b shows); attendee roles and
  RSVP flattened; imports only events the calendar itself organizes or attends.
  CATEGORIES, PRIORITY and X- properties survive as private extended properties.
- **Microsoft Graph** ([[RUN-20261124-hwkyzz]]): its own UIDs (nitya keeps the mapping);
  a pattern model that cannot hold RDATE or THISANDFUTURE (refused) and turns a cancelled
  instance into a deleted one; no URL, ATTACH or X- properties, one alarm; no floating
  time. Patching a series' recurrence or start resets its exceptions, so a driver patches
  only what changed.

Prediction 4 is not met as written. Invitations went out from Graph within seconds and
the acceptance reached the organizer's Outlook.com inbox within 20 s, yet Graph never
applied it to the event's attendee status (two runs, 10 min each). iCloud reported an
invitation delivered that never arrived (the owner's Apple ID is a Gmail address). nitya
should not rely on a provider to process iTIP replies: it reads the mailbox and can
apply REPLYs to its own record itself, a decision for an ADR.

Crate: the harness's own reader (unfolding, content lines, components) plus `rrule` for
expansion was enough for every case; calcard (pinned, MIT/Apache before it moved into
Stalwart) also keeps meaning ([[RUN-20261123-f5xbpv]]); icalendar's writer must not be
used. nitya writes by editing content lines of the stored text, so a full model is not
needed to write.

Also learned: Microsoft rotates refresh tokens on use, so the server keeps the latest in
its own store; the agenix copy only bootstraps.
