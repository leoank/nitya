# nitya

*नित्य: the everyday.*

nitya is a self-hosted server for the day-to-day of life: your calendars, your email and
your tasks in one place you own, with an agent that can do the mundane work for you, but
only through plans you approve. It is meant to replace the vendors' apps one tool at a
time: calendar and email first, then notes, finances, food and health.

What it is built to do, once its first phase is done:

- **Calendars synced both ways** with Google, Microsoft 365 and any CalDAV server, so an
  event moved in nitya moves everywhere, recurring series and time zones included.
- **Meeting polls** that people without an account answer from a link, booked on your
  calendar when you pick a slot.
- **Email** from Gmail, Microsoft 365 and IMAP, read, searched and triaged in one place.
- **Tasks** tied to the events and messages they came from.
- **Agents that ask first.** Agents run as sessions of
  [saarthi](https://leoank.github.io/saarthi/) and hold narrow grants (read the
  calendar, draft a reply); every change they want to make is a plan you approve from
  your phone, and nitya never sends mail on their behalf unless you say so.
- **Your data stays yours.** Everything is encrypted under keys of your own, each device
  has its own identity, and every read and write lands in a signed audit log.

nitya runs as a server on one of your machines and a small node on each of the others,
all on your tailnet; on the phone it is a workspace of the saarthi app.

> **nitya is early.** None of the features on this list run yet. What runs is the core
> they will rest on: the server and node on your tailnet, enrolled devices, your keys,
> the policies and the audit log. [Where it stands](status.md) says what exists today, and
> [Using nitya](using/running.md) shows how to run it.

nitya is built in the open at [github.com/leoank/nitya](https://github.com/leoank/nitya),
with its lab notebook (experiments, runs, decisions) in the same repository.
