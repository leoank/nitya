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

Underneath all of it:

- **Every device has its own identity.** A node, an app install or an agent job enrolls
  with a one-time code and gets a short-lived certificate tied to its machine on the
  tailnet. Every request is signed with the device's key
  ([Running nitya](using/running.md)).
- **Your data is under your keys.** The server stores it encrypted and holds no key that
  opens it, except for the classes you let it sync while you are offline
  ([Your keys](using/keys.md)).
- **Policies decide every request**, and agents get only what a grant names
  ([Who may do what](using/access.md)).
- **Every decision is in a signed log** that your devices check, so a rewritten history
  is noticed ([The audit log](using/audit.md)).

Meeting polls are the one thing outside your tailnet: people you invite answer on a small
page that holds only the poll, and nitya fetches their answers from there.

The design, with the reasons behind each choice, is in the [Design](design/architecture.md)
section.
