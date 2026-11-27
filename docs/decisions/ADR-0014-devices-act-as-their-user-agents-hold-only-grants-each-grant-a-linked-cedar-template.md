# ADR-0014: Devices act as their user; agents hold only grants, each grant a linked Cedar template

Status: accepted (2026-11-27; the owner chose both points below; the schema's details settled by agent:claude for the owner to check)
Date: 2026-11-27

## Context
ADR-0006 makes Cedar the policy engine, with `User`, `Device` and `Agent` principals,
grants as separate actions none of which implies another (ADR-0004), and a `forbid` on
agent writes without an approved plan. Building it ([[AGS-20261127-1x4w3q]]) left two
questions for the owner:
- what a node may do compared with the phone app; the owner chose the same: both act as
  the user;
- how a grant, scoped to an action, a source and a time, is written in Cedar; the owner
  chose one linked template per grant.

Alternatives considered:
- A node narrower than the app (notes, files, notifications and key operations only):
  safer if a machine running agents is taken, but every other use from a node would need
  a grant of its own.
- Grants as attributes of the agent entity read by one policy per action: fewer
  policies, but an audit entry would name the generic policy, not the grant.

## Decision
- **Schema** (`crates/server/policies/nitya.cedarschema`, namespace `Nitya`): `User`;
  `Device` and `Agent` under their `User`, with a `user` attribute; `KeySet`, `Source`
  (with its class), `Item` (under its source) and `Plan`, all under their `User`.
  Actions: `keys.read`, `keys.write`, `plan.approve`, and ADR-0004's grant actions
  (`calendar.read`, `calendar.event.read`, `calendar.write`, `calendar.invite`,
  `mail.search`, `mail.read`, `draft.create`, `task.read`, `task.write`). The writes are
  members of one group, `writes`, used only to require a plan; no grant action is a
  member of another.
- **Policies** (`nitya.cedar`), each with an `@id` that becomes its policy id:
  `devices-act-as-their-user`; `only-the-callers-own-account` (a forbid);
  `agents-never-touch-keys`; `agents-never-approve-plans`;
  `agent-writes-need-an-approved-plan` (a forbid unless the context says the plan was
  approved, which the server sets only after checking the plan's hash).
- **Grants** (`grants.cedar`): a template per grant action,
  `permit (principal == ?principal, action == "<action>", resource in ?resource)`; a
  grant links it to an agent and a source as policy `grant-<id>`. The server links only
  unexpired grants; a grant to an unknown action fails to link.
- **Principals** come from the certificate: kind `agent` is an `Agent`, `node` and `app`
  are `Device`s, identified by their device key, which renewal keeps.
- **Checked** strictly against the schema when the server starts (it refuses to start
  otherwise) and by the server's tests in `nix flake check`. Every decision returns the
  ids of the policies that made it, for ADR-0007's audit entry.
- Draft creation counts as a write and needs a plan, the stricter reading of ADR-0004.

## Consequences
- The `Keys` service asks Cedar before every call: agents are refused keys.
- Until phase 5 stores grants, the server links none, so agents are refused everything.
- A grant's time scope is enforced by the server dropping expired grants, not by Cedar.
- A node that is taken can do what the owner can, short of the account's keys when it
  holds none; revocation (ADR-0010) is the remedy.
