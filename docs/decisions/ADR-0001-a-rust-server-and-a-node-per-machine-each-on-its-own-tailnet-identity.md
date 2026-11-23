# ADR-0001: A Rust server on ghost001 and a node per machine, each on its own tailnet identity, independent of saarthi

Status: accepted
Date: 2026-11-23

## Context
nitya is to replace the owner's user-activity tools, starting with calendar and email, and
to give agents context. It needs a place that syncs and holds the data, something on each
machine for agents and the desktop, and a phone client. saarthi already solved reaching
a hub from nodes and a phone across tailnets: every part embeds its own tsnet identity
(saarthi ADR-0048, ADR-0049), and its hub relays local services such as yajna to the phone
(saarthi ADR-0052). Asked on 2026-11-23, the owner first chose to put nitya behind
saarthi's relay, then reversed it: "keep nitya separate, but mimic how saarthi uses
tailnets for the server and node. This will keep things simple for now."

Alternatives considered:
- nitya as a local service behind a saarthi node, reached through the hub's relay like
  yajna: least new infrastructure, but nitya would depend on saarthi's hub for every
  request and the hub would see nitya's traffic in plain text.
- Both an own listener and a saarthi service: two paths to secure and test.
- One server only, no per-machine part: no place for local agent context, desktop
  notifications or file sync.

## Decision
- A **nitya server**, one Rust binary on ghost001 (the box that runs the saarthi hub), with
  an embedded tsnet identity serving nitya's API on the tailnet, the way saarthi's hub
  does. It holds the store, the sync engine, policies and the audit log.
- A **nitya node** on each of the owner's machines, a small Rust daemon with its own tsnet
  identity, the way saarthi's node does: the agent context bridge, desktop notifications,
  and notes and files sync.
- **Rust**, like saarthi, reusing saarthi's tailnet crate as sandhi does (copied at first,
  shared once stable) and NixOS and home-manager modules from this repo's flake.
- **Multi-account by design**: one person uses it now, but every record belongs to an
  account and nothing assumes there is only one.

## Consequences
- nitya runs and is reachable without saarthi; saarthi is a peer it calls for agent sessions
  (ADR-0004), not a transport.
- nitya carries its own enrollment, device identity and auth (ADR-0005) instead of
  inheriting saarthi's tokens.
- A second set of tailnet identities on the fleet; sandhi may later carry them.
