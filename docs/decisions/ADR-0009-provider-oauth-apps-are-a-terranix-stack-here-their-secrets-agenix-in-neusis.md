# ADR-0009: Provider OAuth apps are a terranix stack in this repo; their secrets are agenix files in neusis

Status: accepted
Date: 2026-11-23

## Context
nitya needs an OAuth app at Google and one at Microsoft before any provider leg of
[[EXP-20261123-bhfzfs]] or [[EXP-20261123-bhfz49]] can run, and later for the channel
drivers. The owner manages cloud infrastructure as terranix stacks in neusis (neusis
ADR-0010: OpenTofu with providers pinned by nixpkgs, state encrypted and committed,
operator variables from `~/.config/neusis/tf.env`) and secrets with agenix-rekey there.
On 2026-11-23 the owner asked whether the OAuth clients could be made that way, and chose
where the stack and the secrets live.

What the providers allow (checked 2026-11-23):
- Microsoft: an Entra app registration, its audience, redirect URIs and delegated Graph
  scopes are all resources of the `azuread` provider.
- Google: the project and its APIs are resources of the `google` provider, but there is no
  API for the consent screen or for a Desktop OAuth client; the only programmatic client
  API, the IAP OAuth Admin API, shut down on 2026-03-19 and made IAP-only clients anyway.
  A tool that drives a signed-in browser through the console exists (gclientid) and was
  not chosen: brittle, and outside Google's terms for automation.

Alternatives considered: the stack in neusis beside `oci-anklab` and `cloudflare-leoank`
(no new input here, all cloud infra in one place); agenix-rekey set up in this repo with
recipients copied from neusis (two places to rekey when a host changes).

## Decision
- The stack `nitya-oauth` lives here, in `nix/infra/nitya-oauth.nix`, through neusis's
  `flakeModules.terranix` and its `state-encryption` module, with the `google` and
  `azuread` providers; its encrypted state is committed under `infra/nitya-oauth/`.
- Microsoft: a public-client app registration for personal and work accounts, v2 tokens,
  a loopback redirect, delegated `Calendars.ReadWrite`, `Mail.ReadWrite`, `User.Read`,
  `offline_access`; no client secret exists.
- Google: the project and the Calendar and Gmail APIs are declared; the consent screen
  (published, so refresh tokens do not expire after 7 days) and the Desktop client are
  made by hand, following the stack's `google_manual_steps` output.
- Secrets (the Google client JSON, refresh tokens from the one-time consent) are agenix
  files in neusis, rekeyed for the hosts that need them (ghost001 first); this repo holds
  none, and nitya's modules take file paths.

## Consequences
- nitya's flake depends on neusis and terranix. neusis's terranix bundle declares its
  input through flake-file, which a hand-written flake does not have; `flake.nix` stubs
  that option until neusis's bundle works in plain flake-parts flakes as its default
  bundle does.
- neusis's `mkValidateCheck` pins the OCI and Cloudflare providers, so this repo has its
  own `tofu validate` check.
- The operator's `tf.env` gains `TF_VAR_google_project_id` and `TF_VAR_azure_tenant_id`.
- Google's client stays a manual step until Google offers an API for it.
