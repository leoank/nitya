---
id: AGS-20261124-hk9hbs
type: agent_session
schema_version: 1
title: "Apply the nitya-oauth stack with the owner: commit its state, record the Azure and Google lessons in the Deployment docs"
model: "claude-opus-5-5"
harness: "claude-code"
started: 2026-11-24T16:01+00:00
finished: 2026-11-24T16:02+00:00
created_entities: []
edited_entities: []  # docs/guide deployment pages, infra/nitya-oauth state
outcome: done             # done | failed | aborted | pending
created: 2026-11-24
author: agent:claude
---

## Task

The owner ran the nitya-oauth stack (ADR-0009) for the first time; commit its encrypted
state and write what it took into the Deployment section of the guide.

## What happened

- Azure: the owner's Cambridge login saw only the university tenant; their Hotmail
  account had no directory, so `az` landed in Microsoft Services and Graph answered
  `User was not found` (the azuread provider calls this a missing context deadline). The
  owner made their own tenant with a member admin; the plan then read Graph.
- `az` 2.91 crashes in its tenant picker without a subscription
  (`AZURE_CORE_LOGIN_EXPERIENCE_V2=off`); `az account tenant list` needs an extension the
  nix CLI cannot install (`az rest` on `/tenants`).
- Google: the project `nitya-515713` had been made in the console, so the apply hit
  `409 alreadyExists`; imported. ADC without a quota project hit the shared gcloud
  quota (429); `set-quota-project` to the nitya project.
- Commits 216a5ab (encrypted state, only `encrypted_data` in the file) and 9300321
  (deployment docs: a tenant of your own, the login checks, the quota project, import).
  `nix flake check` passed on each staged tree; the new anchor renders in the built docs.
- Next for the owner: the four Google console steps, then `nitya-consent` into
  `nityaGoogleClient.age`, `nityaGoogleToken.age`, `nityaMicrosoftToken.age`.
