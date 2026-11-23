# Where it stands

nitya is in **phase 0: spikes and design**. The design is written down and the
assumptions it rests on are being tested against the real services before anything is
built on them.

What exists today:

- The design: the [architecture](design/architecture.md) and its
  [decision records](design/decisions/README.md).
- The sign-in apps at Google and Microsoft, as code you can apply
  ([Provider sign-in apps](deployment/provider-apps.md)), and
  [`nitya-consent`](reference/nitya-consent.md), which signs an account in once and
  stores its refresh token with agenix ([Consent and refresh tokens](deployment/consent.md)).
- The first experiment, on whether calendars survive a round trip through each provider
  without losing anything ([[EXP-20261123-bhfzfs]]). Its first results: recurring series,
  exceptions, time zones and attendees all survive a CalDAV server intact; Google and
  Microsoft are next.

Not yet: the server, the node, the app workspace, sync, polls and agents. The phases that
build them are on the [project page](https://github.com/leoank/nitya/blob/main/notes/README.md).
