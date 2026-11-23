---
id: AGS-20261123-fxw2b1
type: agent_session
schema_version: 1
title: Commit and push the experiment; can OAuth clients for Google and Microsoft be made with terranix and agenix as neusis manages cloud infra
model: claude-opus-5-5
harness: claude-code
started: 2026-11-23T14:30+00:00
finished: 2026-11-23T14:38+00:00
created_entities: []
edited_entities: [PRJ-0001, EXP-20261123-bhfz49, REV-20261123-f1633e]
outcome: done
created: 2026-11-23
author: agent:claude
---

## Task
The owner asked to commit and push the experiment's work, and whether the OAuth clients
for Google and Microsoft can be created with terranix and agenix the way neusis manages
cloud infrastructure.

## What happened
- Committed `4ae2432` (the notebook check copies `spikes/`) and `9ef2ea1` (the runs and
  notes of [[EXP-20261123-bhfzfs]]), each checked from a copy of the staged tree; pushed.
- Read neusis (read only; its local `main` is ahead 2 and behind 2 of origin, left alone):
  ADR-0010, `features/flake/_terranix-defaults.nix`, `terranixModules/`, `infra/`.
- Checked the providers: Microsoft's Entra app is fully declarable with `azuread`;
  Google has no API for the consent screen or a Desktop OAuth client, and the IAP OAuth
  Admin API shut down on 2026-03-19.
- The owner chose the stack here and the secrets in neusis. Built `nix/infra/nitya-oauth.nix`
  on `neusis.flakeModules.terranix` and `state-encryption`, a `tofu validate` check, a
  `nitya-oauth-login` shell with gcloud and az, `infra/.gitignore` as neusis's; recorded
  ADR-0009. `nix flake check` passes; nothing applied.
- Worked around neusis: its terranix bundle declares `flake-file.inputs`, so `flake.nix`
  stubs that option; its `mkValidateCheck` hardcodes OCI and Cloudflare providers. Both
  are small fixes for neusis, left for the owner.
- Not committed: the stack, ADR-0009 and these notes; the owner asked only for the earlier
  work to be committed.
