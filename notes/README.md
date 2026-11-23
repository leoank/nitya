---
id: PRJ-0001
type: project
schema_version: 1
title: nitya - one self-hosted server for the day-to-day of life
status: active            # active | paused | concluded | moved | archived
code: .               # code/<name> here, or a repository URL once it moves out
created: 2026-11-23
author: ank
tags: []
---

## Introduction
Can nitya, a self-hosted multi-account server with strong cryptography and auditable
role-based access, gradually replace every user-activity tool in my computing stack,
starting with calendar and email, while giving my agents (through saarthi) enough context
to automate most mundane tasks?

nitya (नित्य, the everyday, the constant) is the owner's own server for the day-to-day of
life: calendar, email, tasks, and later notes, finances, food and health. It is meant to
replace the vendors' apps one tool at a time, and to hold that data in one place where the
owner's agents can use it to do the mundane work, under grants the owner controls and
with every access on record. It is written rather than assembled from existing tools because
none of them combines two-way sync with the big providers, per-user encryption, auditable
access control, and agents that act only through plans a person approves (prior art in
the reading notes: @msgvault, @calcom-caldiy, @rallly, @stalwart and others).

A nitya server runs on ghost001 beside the saarthi hub; a small nitya node runs on each
of the owner's machines; both have their own tailnet identities, as saarthi's parts do.
The phone client is the Nitya workspace in saarthi's app, and agents are saarthi
sessions that nitya starts. One person uses it now; the server supports several accounts.

Phase 1, about three months, no hard deadline: calendars synced two-way with Google,
Microsoft 365 and CalDAV; meeting polls that outside people answer; email read, searched
and triaged; tasks tied to them; agents acting through approved plans; the node bridging
agent context, desktop notifications and notes and files sync. Out of scope for now:
finances, food and health, and being a mail server.

## Progress
The project's phases, one line each, ticked when the phase is done; each phase's page holds
its goal and its own checklist. `yajna new phase` adds one; `yajna tasks` lists the open
items with the rest.
- [ ] [[PHS-20261123-bhenjn]] Phase 0: spikes and design
- [ ] [[PHS-20261123-bhen36]] Phase 1: core: server and node on tsnet, device identities, per-user keys, audit log, RBAC
- [ ] [[PHS-20261123-bhen6w]] Phase 2: calendar sync and tasks
- [ ] [[PHS-20261123-bhend2]] Phase 3: email: read, search, triage
- [ ] [[PHS-20261123-bhensm]] Phase 4: meeting polls
- [ ] [[PHS-20261123-bheny1]] Phase 5: agents: nitya spawns saarthi sessions, phone approvals, then MCP through the node
- [ ] [[PHS-20261123-bhenge]] Phase 6: the Nitya workspace in saarthi's app
- [ ] [[PHS-20261123-bhepkn]] Node: notes and files sync

## Open questions
- What does the node's notes and files sync cover, and is it built or borrowed
  ([[PHS-20261123-bhepkn]])?
- Automatic agent rules that run without approval: deferred; the baseline is approval for
  every write (ADR-0004), revisited after use.
- Retention of read entries in the audit log (ADR-0007).
- Does Cedar model sharing with other people well enough, or does that want a
  relationship engine (ADR-0006)?

## Design
The architecture is `docs/architecture.md` and its decision records are
`docs/decisions/`; here, only the few choices
a reader must know before touching the code.
- A Rust server and a node per machine, each with its own tailnet identity, independent
  of saarthi; saarthi is a peer nitya calls to spawn agent sessions (ADR-0001, ADR-0004).
- nitya's own store holds raw iCalendar and MIME with provider ids and cursors as the
  record; indexes are derived; sync polls (ADR-0003).
- Agents hold grants of separate actions, draft rather than send, and write only through
  plans the owner approves; policy is Cedar; every read and write goes into a signed,
  hash-chained log (ADR-0004, ADR-0006, ADR-0007).
- Per-device keys with short-lived certificates; a per-user key hierarchy; background
  work through a consented, logged sync delegate (ADR-0005).

## Lessons
What was tried and dropped, and why, so nobody tries it again.
- From yajna's Google sync ([[yajna/EXP-20261021-as4202]], yajna ADR-0011): code tested
  only against a fake of a Google API behaved differently on the real one; test every
  provider driver against the real service early. The desktop OAuth flow with a loopback
  redirect and PKCE gave a refresh token for the personal account, with the narrowest
  scope, and the token belongs in neusis secrets, never in a repo.
- From the same work ([[yajna/EXP-20261023-ftnatx]]): a provider's export is not a
  faithful round trip of what was put in; assume nothing round-trips until
  [[EXP-20261123-bhfzfs]] shows it.
- From prior art, not to repeat: credential encryption that fails open when no key is
  configured, and AES-CBC without authentication (@calcom-caldiy); OAuth tokens in
  plaintext columns and agent tools that send or bulk-delete with no approval (Zero);
  approval queues so slow they are abandoned (@inbox-zero).

## Experiments
One line per experiment after its link, what it asks and what came of it; `yajna validate`
rejects a bare link here. Newest last.
- [[EXP-20261123-bhfzfs]] do recurring series, exceptions, zones and attendees round-trip through Google, Graph and CalDAV; planned, first
- [[EXP-20261123-bhfzeh]] does the per-user key hierarchy with a sync delegate allow offline work, full audit and clean revocation; planned
- [[EXP-20261123-bhfz49]] do Google and Microsoft refresh tokens survive 30 days on a headless server; planned, start early
- [[EXP-20261123-bhfz7e]] can nitya spawn a saarthi session with a grant and execute its plan only after phone approval; planned

## Decisions
Settled in the interview of 2026-11-22/23 ([[AGS-20261123-bhdnxc]]); each record in
`docs/decisions/` gives the alternatives.
- ADR-0001, server and nodes on their own tailnet identities, not behind saarthi's relay:
  the owner's choice, to keep nitya separate and simple, reusing saarthi's tailnet pattern.
- ADR-0002, the phone client is the Nitya workspace in saarthi's app: the app already
  reserves the tab, and one app keeps one notification service and identity store.
- ADR-0003, own store as the record, polling sync: full control of encryption and policy,
  offline search for agents; push would need a public endpoint.
- ADR-0004, scoped grants, drafts not sends, approved plans; spawning first, MCP next:
  msgvault's model, so an agent's mistake is a draft or a refused plan, not a sent mail.
- ADR-0005, device keys, a per-user key hierarchy and a consented sync delegate, no MLS:
  meets per-user keys and background sync at once; MLS solves a problem nitya does not have.
- ADR-0006, Cedar: embedded in the binary, grants that never imply each other, and
  decisions that name their policies for the audit log.
- ADR-0007, a signed, hash-chained audit log: access must be auditable, including by
  devices that do not trust the server's database.
- ADR-0008, poll respondents reach a Cloudflare Worker with D1 holding only poll data,
  which nitya pushes to and pulls from (accepted 2026-11-23): the public side of polls
  without exposing ghost001 or running a machine to patch; Cloudflare sees a poll's
  invitees and choices while it is open.
- ADR-0009, provider OAuth apps are the `nitya-oauth` terranix stack in this repo, through
  neusis's stack runner, and their secrets agenix files in neusis (the owner's choice,
  2026-11-23): Microsoft's app is declared whole; Google's project and APIs are declared,
  its consent screen and Desktop client stay manual since Google has no API for them.
