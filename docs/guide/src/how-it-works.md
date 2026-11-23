# How it works

nitya has three parts, each with its own identity on your tailnet.

```text
 phone                         your server (e.g. ghost001)           each machine
 ┌──────────────────┐ tailnet  ┌────────────────────────────┐ tailnet ┌──────────────────┐
 │ saarthi app      │────────►│ nitya server               │◄────────│ nitya node       │
 │  Nitya workspace │          │  store, sync, policy, audit│         │  agent context   │
 └──────────────────┘          └──────────┬─────────────────┘         │  notifications   │
                                          │ polls                     │  file sync       │
                          Google · Microsoft 365 · IMAP · CalDAV      └──────────────────┘
```

- **The server** holds your data and keeps it in sync with your providers. It stores
  events and messages exactly as the providers sent them, encrypted, and builds its
  search indexes from them. It checks every request against your policies and records it
  in the audit log.
- **A node** runs on each of your machines. It gives agents on that machine the context
  they are allowed to see, shows reminders, and keeps notes and files in sync.
- **The phone** reaches the server directly from the Nitya workspace of the saarthi app:
  calendar, mail, tasks, polls, and the plans agents are waiting for you to approve.

Agents are [saarthi](https://leoank.github.io/saarthi/) sessions. nitya asks saarthi to
start one for a job (triage the inbox, find a time for a meeting) and hands it a grant
that covers only that job. The agent reads what the grant allows and proposes a plan;
nothing changes until you approve it.

Meeting polls are the one thing outside your tailnet: people you invite answer on a small
page that holds only the poll, and nitya fetches their answers from there.

The design, with the reasons behind each choice, is in the [Design](design/architecture.md)
section.
