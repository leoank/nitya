# Who may do what

Every request names a device by its certificate, and nitya's policies decide what it may
do. The policies are written in [Cedar](https://www.cedarpolicy.com/) and live with the
server's code (`crates/server/policies/`). The server checks them against their schema
when it starts, and refuses to start if they don't validate.

- **Your devices act as you.** A node or an app may do anything with its own account:
  read and write data, manage keys, approve plans. Nothing with another account.
- **Agents hold only grants.** A device enrolled with `--kind agent` is an agent. It may
  do exactly what a grant names: one action (read the calendar, create a draft) on one
  source. No grant implies another: permission to write a calendar does not let an
  agent read it.
- **Agents never touch keys or approve plans**, whatever their grants say.
- **Every agent write needs a plan you approved.** An agent proposes, you approve from
  your phone, and only then may it write.

Grants are not stored yet; that comes with agents in a later phase. Until then agents are
refused everything:

```text
$ nitya-node … keys status          # on an agent's device
… message: "audit.read refused: no policy permits it"
```

Every decision, allowed or refused, is in the [audit log](audit.md) with the policies
that made it.
