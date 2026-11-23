---
id: PHS-20261123-bhend2
type: phase
schema_version: 1
title: "Phase 3: email: read, search, triage"
project: PRJ-0001
status: planned           # planned | active | done | abandoned
since: null               # the commit it started at: yajna phase start
until: null               # the commit it finished at: yajna phase done
created: 2026-11-23
author: agent:claude
tags: []
---

## Goal
Read, search and triage every mailbox from nitya. Done when the owner's Gmail, Microsoft 365
and IMAP mailboxes are synced and searchable, and triage (labels or folders, archive,
drafts) works from the app; nitya does not send without an explicit action.

## Progress
One item per piece of work, ticked when it lands; add items as the plan grows and keep the
done ones. The project page's *Progress* lists this phase on one line.

- [ ] 1. Gmail driver: history-based sync, raw MIME stored (@gmail-sync, @msgvault)
- [ ] 2. Microsoft Graph mail driver
- [ ] 3. IMAP driver with CONDSTORE/QRESYNC (@rfc7162-condstore-qresync); evaluate pimalaya crates (@himalaya)
- [ ] 4. Full-text index, derived and rebuildable
- [ ] 5. Triage actions written back to the provider; drafts

## Notes
