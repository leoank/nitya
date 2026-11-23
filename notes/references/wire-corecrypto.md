---
type: reference
schema_version: 1
citekey: wire-corecrypto
title: "core-crypto: MLS and Proteus for Wire clients"
authors: Wire Swiss
year: 2026
doi: null
status: skimmed
role: method
created: 2026-11-23
author: agent:claude
tags: []
---

## What it says
Surveyed by an agent on 2026-11-22/23 from the source and docs ([[AGS-20261123-bhdnxc]]); [V] checked in source or docs, [I] inferred.

Wire's client cryptography in Rust (GPL-3.0-only; its `e2e-identity` crate is MPL-2.0) [V].
- Layers: generated Kotlin/Swift/TS bindings, `crypto-ffi`, `crypto` (one API over MLS,
  through a fork of openmls, and Proteus), `e2e-identity`, `keystore`; every state change
  commits atomically through a transaction context [V].
- The keystore is SQLCipher keyed with a raw 32-byte key the app supplies from the
  platform's secure storage; CoreCrypto never creates or stores it; the key string is
  zeroized; rekey is supported [V].
- Each device has its own signature key; E2EI gets it an X.509 certificate from ACME through
  a DPoP challenge (signed with the device key) and an OIDC challenge; CRLs are fetched and
  cached; removing a device is an MLS remove commit to a new epoch [V].
- Server-side reading is Wire's legal hold: a consented, visible, admin-removable virtual
  device in the user's conversations (@wire-legalhold) [I].

## What it means for this work
- Not linked into nitya (GPL); its patterns shape ADR-0005: per-device keys, short-lived
  device certificates from nitya acting as a small CA, a database key supplied from outside
  the store, and the sync delegate modelled on legal hold.
- MLS is not used: it solves group messaging among parties who distrust the server, and
  nitya's server must read the data.
- [[EXP-20261123-bhfzeh]] tests the resulting key hierarchy.
Informs [[PRJ-0001]].

## Doubts and follow-ups
- None yet.
