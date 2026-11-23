# ADR-0008: Poll respondents reach a Cloudflare Worker that holds only poll data, never nitya

Status: proposed
Date: 2026-11-23

## Context
Meeting polls ([[PHS-20261123-bhensm]]) are answered by people outside the owner's
tailnet, and nitya is reachable only on the tailnet (ADR-0001). The interview left the
public path open; on 2026-11-23 the owner asked whether Cloudflare Workers or serverless
functions could carry it.

Alternatives considered:
- Tailscale Funnel on ghost001 for the poll routes only: no new vendor, but it puts the
  machine holding every account's data on the public internet, and one routing mistake
  exposes more than polls.
- A small VPS as a public edge: the same separation as a Worker, with a machine to patch.
- Email replies only (iTIP REPLY, or parsed answers): nothing public, but a poor
  experience for picking among many slots, and parsing replies is fragile.

## Decision
- A Cloudflare Worker serves the poll pages and a small API; Cloudflare D1 holds only what
  a poll needs: the offered slots, a title the owner writes for outsiders, invitee
  capability tokens stored hashed, and responses. No calendar items, mail or account data
  leave nitya.
- nitya talks to the Worker; the Worker never reaches nitya. The server pushes a poll out
  and pulls its responses over outbound HTTPS, signing each request with its Ed25519 key,
  which the Worker verifies (ADR-0005); pulling on a schedule matches ADR-0003.
- Tokens follow Rallly (@rallly): one per invitee and one per response, from a CSPRNG,
  rotatable and revocable; an email typed by a respondent is never trusted.
- Once a poll is finalised and the event is booked in nitya, its data in D1 is deleted;
  nitya keeps the record and the audit entries.
- Written in Rust with `workers-rs` if its WebAssembly build is comfortable, else
  TypeScript; deployed with `wrangler` from the dev shell.

## Consequences
- A vendor dependency in an otherwise self-hosted system, limited to polls; Cloudflare
  sees invitees' names, emails and choices and the offered slots while a poll is open.
  Encrypting responses in the browser to nitya's key would hide them from Cloudflare but
  also from other respondents; not done unless the owner asks.
- Free-tier limits are far above one person's polls.
- Invitations still have to be sent by mail from the owner's account, an explicit action by
  the owner, not an agent (ADR-0004).
- To be accepted by the owner before phase 4 starts.
