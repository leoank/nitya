# Where it stands

nitya is in **phase 1: the core**. The server and the node run on a tailnet with the
security everything else will rest on. There are no calendars, mail or agents yet.
Phase 0, the design and its spikes, is done except for one experiment that takes a month.

What runs today ([Using nitya](using/running.md)):

- **The server and the node**, each on its own tailnet identity, talking gRPC on the
  tailnet only.
- **Device enrollment**: one-time codes, 24-hour certificates bound to the device's tailnet
  node, every request signed by the device, renewal, revocation
  ([Running nitya](using/running.md)).
- **Your keys**: an account's keys made on its first device, shared with new devices only
  when you approve them by fingerprint, recovered from a passphrase, rotated after a
  revocation, and delegated to the server class by class ([Your keys](using/keys.md)).
- **The store**: one SQLite database holding accounts, certificates, key sets, the audit
  log, and the channels and sealed items that sync will fill.
- **Policies**: Cedar decides every request; your devices act as you, agents only by
  grant ([Who may do what](using/access.md)).
- **The audit log**: every decision signed and chained, and checked by your devices
  ([The audit log](using/audit.md)).
- **The sign-in apps** at Google and Microsoft, as code you can apply
  ([Provider sign-in apps](deployment/provider-apps.md)), and
  [`nitya-consent`](reference/nitya-consent.md), which signs an account in once and
  stores its refresh token with agenix ([Consent and refresh tokens](deployment/consent.md)).

What the experiments found: calendars keep recurring series, exceptions and time zones
through every provider, but Google and Microsoft drop some fields a CalDAV server keeps
([[EXP-20261123-bhfzfs]]). The key hierarchy lets the server work while you are offline
without reading what you did not delegate, provided revocation also rotates the data keys
([[EXP-20261123-bhfzeh]]). Whether the provider tokens last a month on a headless server
is being measured ([[EXP-20261123-bhfz49]]).

Next: the NixOS and home-manager modules, and nitya running on its own server. After
that, phase 2 brings calendar sync and tasks. The phases are on the
[project page](https://github.com/leoank/nitya/blob/main/notes/README.md).
