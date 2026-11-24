---
id: EXP-20261123-bhfzfs
type: experiment
schema_version: 1
title: Do recurring series with exceptions, all-day events, time zones and attendees round-trip between Google Calendar, Microsoft Graph, CalDAV and a store of raw iCalendar with provider ids, without loss?
project: PRJ-0001
status: active            # planned | active | paused | concluded | abandoned
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
- [[RUN-20261124-hwkxry]] compute run, not yet described
- [[RUN-20261124-hwkyzz]] compute run, not yet described

## Conclusion
Written once, by a human, when status becomes `concluded`.
