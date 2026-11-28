---
id: EXP-20261127-mqzh92
type: experiment
schema_version: 1
title: Does Google Calendar's incremental sync report edits, moved and cancelled instances and deletions, ask for a full sync with 410, and refuse stale writes with 412 under If-Match?
project: PRJ-0001
status: concluded         # planned | active | paused | concluded | abandoned
outcome: positive         # null | positive | negative | inconclusive | infra
influenced_by: [EXP-20261123-bhfzfs]
created: 2026-11-27
author: agent:claude
protocols: []
materials: []
tags: []
review: pending
---

## Question
Phase 2 item 2's Google Calendar driver (ADR-0017's `Driver`, two-way) rests on how
Google's `events.list` behaves between syncs, which [[EXP-20261123-bhfzfs]] never
exercised: it wrote and read whole events by `iCalUID`. Does a sync token report every
change, in a form the engine can store as one item per Google event, and can a write be
refused when the event changed at Google since nitya last read it?

## Hypothesis and predictions
H1: `events.list` with `singleEvents=false` gives a full listing that ends in a
`nextSyncToken`, and listing with that token later returns exactly what changed.

1. A full listing (no `showDeleted`) holds the series' master, its moved instance and its
   cancelled instance (as a stub with `status: cancelled` and `recurringEventId`), and
   `nextSyncToken` only on the last page (`maxResults=2` to force pages).
2. After edits at Google (a single event retitled, a series instance moved, another
   cancelled, a single event deleted, one inserted), the token's listing returns those five
   and nothing else: the deleted event as `status: cancelled` without `recurringEventId`,
   the moved and cancelled instances with it. A listing with the newest token is empty.
3. A garbled sync token is answered with 410 Gone (the engine's `CursorExpired`). If it is
   400 instead, the driver must treat that as an expired cursor too.
4. `events.update` and `events.delete` with `If-Match` of a stale ETag answer 412 and
   change nothing; with the current ETag they succeed. If Google ignores `If-Match`, the
   driver must compare ETags itself, racing the provider.
5. `events.insert` with an `iCalUID` keeps it.

## Plan
1. A script in `spikes/google-sync/` makes a throwaway calendar on the owner's account
   (the tokens from [[EXP-20261123-bhfz49]]'s consent, as the round trip used them), adds
   two single events and a weekly series, moves one instance and cancels another, and
   lists it in full with `maxResults=2`.
2. Edits as in prediction 2, then lists with the token, and again with the new token.
3. A garbled token; stale and current `If-Match` on update and delete; an insert with an
   `iCalUID`.
4. Writes the redacted listings and one row per prediction to the run's directory;
   deletes the calendar.

## Success criteria
Positive when predictions 1 to 5 hold: the driver can keep a Google sync token as its
cursor, map each listed event to an upsert or a delete, ask for a full sync on 410, and
write with `If-Match`. Each prediction that fails names what the driver does instead.

## Runs
One line per run after its link, what was tried and what it showed; `yajna validate`
rejects a bare link here. Metric runs also land in results.tsv.
- [[RUN-20261127-mr3rsk]] throwaway calendar: tokens report every change, but a changed
  instance re-sends the whole series; garbled token 410 `fullSyncRequired`; stale
  If-Match 412 on update and delete; insert keeps the iCalUID

## Conclusion
*Draft by agent:claude for the owner's review.*

H1 holds, with prediction 2 in a weaker form ([[RUN-20261127-mr3rsk]]): a sync token's
listing holds every change since the token, but not only those, because any change to a
series' instance lists the master and all its exceptions again. The Google driver can
therefore:
- keep the sync token (or, while paging, the page token) as the channel's cursor, and
  take a 410 as `CursorExpired`, the full sync ADR-0017 already does;
- map each listed event to one item keyed by Google's event id: a `cancelled` event with
  no `recurringEventId` is a delete, a cancelled instance is an item of its own (a stub
  that cancels one occurrence), everything else an upsert;
- skip an upsert whose ETag matches the stored one, so the re-sent series costs nothing;
- write updates and deletes with `If-Match` of the stored ETag and treat 412 as a
  conflict, never overwriting a change made at Google since nitya read it;
- create events with `events.insert` and the `iCalUID` nitya gives them.
