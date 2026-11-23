# ADR-0006: Cedar is the policy engine

Status: accepted
Date: 2026-11-23

## Context
The owner asked to reuse or learn from open-source authorization rather than invent it.
Compared (survey in [[AGS-20261123-bhdnxc]]): Cedar (@cedar, Rust crate, Apache-2.0),
casbin-rs (weak explanations), Oso (open-source repo archived), OpenFGA, SpiceDB and Ory
Keto (Zanzibar-style, separate Go services), Permify (AGPL), Biscuit (@biscuit, capability
tokens with Datalog).

## Decision
Cedar, embedded in the nitya server. Principals `User`, `Device` and `Agent` (an agent's
parent is its user); every grant of ADR-0004 is its own action with no `memberOf` link
between them, so write never implies read; a `forbid` on agent writes unless the request
carries the hash of a plan a human approved; each decision's policy ids go into its audit
entry. Biscuit stays an option for narrowed, offline-verifiable agent credentials later.

## Consequences
- One binary, no authorization service to run; policies and their schema live in this repo
  and are validated in CI.
- Relationship-heavy sharing (other people's calendars) may need entity modelling Cedar
  handles less naturally than a Zanzibar engine; revisit if sharing grows.
