# ADR-0002: The Android client is the Nitya workspace in saarthi's app

Status: accepted
Date: 2026-11-23

## Context
The owner wants nitya on the phone and named saarthi's Android app as its home. saarthi's
app already reserves a *Nitya* tab in violet among its workspaces (saarthi ADR-0068) and
has a Rust core under Kotlin through UniFFI with an embedded tsnet identity (saarthi
ADR-0049). Under ADR-0001 nitya is not behind saarthi's hub.

Alternatives considered: a separate nitya app (a second tailnet identity, notification
service and settings on the phone for one person); a web UI only (no notifications, no
offline, no device key in the Android Keystore).

## Decision
nitya's phone client is the Nitya workspace in saarthi's app, built in the saarthi repo.
It talks to the nitya server directly over the tailnet with a device identity of its own
(ADR-0005), not through saarthi's hub. Approvals of agent plans (ADR-0004) arrive there.

## Consequences
- App work lands in saarthi's repo and release train; nitya's API must be callable from
  saarthi's Rust core (a nitya client crate it can depend on).
- The app holds identities for two servers; their tailnets may differ.
