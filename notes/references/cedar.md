---
type: reference
schema_version: 1
citekey: cedar
title: "Cedar: a policy language and evaluation engine"
authors: Cedar contributors
year: 2026
doi: null
status: skimmed
role: tool
created: 2026-11-23
author: agent:claude
tags: []
---

## What it says
Surveyed by an agent on 2026-11-22/23 from the source and docs ([[AGS-20261123-bhdnxc]]); [V] checked in source or docs, [I] inferred.

AWS's policy language and engine, an embeddable Rust crate (Apache-2.0, active) [V]. RBAC
and ABAC with some relationships through entity hierarchy; `permit` and `forbid`, forbid
wins; schema validation; responses name the policies that decided them [I].
Compared against casbin-rs, Oso (open-source repo archived), OpenFGA, SpiceDB, Permify
(AGPL), Ory Keto and Biscuit (@biscuit): the Zanzibar-style engines run as separate
services built for large relationship graphs [V/I].

## What it means for this work
- nitya's policy engine (ADR-0006): principals `User`, `Device`, `Agent` (child of its
  user); each grant (`calendar.read`, `draft.create`, `calendar.write`, ...) a separate
  action with no implication between them; a `forbid` on writes unless the request carries
  a human-approved plan; the deciding policy ids written into each audit entry.
- Biscuit is a later option for narrowed, offline-verifiable agent credentials.
Informs [[PRJ-0001]].

## Doubts and follow-ups
- None yet.
