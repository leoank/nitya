# Where it stands

nitya is in **phase 2: calendar sync and tasks**. Phase 1 built the core: the server
runs on its own machine and a node on the owner's, on a tailnet, with the security
everything else rests on. Google calendars sync both ways; Microsoft and CalDAV
calendars can't be connected yet, and there is no mail and there are no agents.
Phase 0, the design and its spikes, is done except for one experiment that takes a month.

What runs today ([Using nitya](using/running.md)):

- **The server and the node**, each on its own tailnet identity, talking gRPC on the
  tailnet only, deployed by a NixOS module and a home-manager module
  ([The server and node modules](deployment/modules.md)).
- **Device enrollment**: one-time codes, 24-hour certificates bound to the device's tailnet
  node, every request signed by the device, renewal, revocation
  ([Running nitya](using/running.md)).
- **Your keys**: an account's keys made on its first device, shared with new devices only
  when you approve them by fingerprint, recovered from a passphrase, rotated after a
  revocation, and delegated to the server class by class ([Your keys](using/keys.md)).
- **The store**: one SQLite database holding accounts, certificates, key sets, the audit
  log, and the channels and sealed items that sync will fill.
- **The sync engine**: channels synced every minute while you consent, or at once with
  `nitya-node sync now` ([Channels and sync](using/sync.md)).
- **Google calendars, both ways**: an account signed in from a node with
  `nitya-node channels add google`, each chosen calendar a channel; events read, added,
  edited and deleted with `nitya-node events`, written back to Google at once, with what
  Google can't hold and changes that would overwrite one made at Google refused
  ([Channels and sync](using/sync.md#reading-and-changing-events)).
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

Next: the Microsoft Graph and CalDAV drivers, two-way. The phases are on the
[project page](https://github.com/leoank/nitya/blob/main/notes/README.md).
