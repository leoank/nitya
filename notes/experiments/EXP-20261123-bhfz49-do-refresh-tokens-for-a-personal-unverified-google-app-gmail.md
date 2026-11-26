---
id: EXP-20261123-bhfz49
type: experiment
schema_version: 1
title: Do refresh tokens for a personal, unverified Google app (Gmail and Calendar scopes) and an Entra app for Microsoft 365 stay valid on a headless server for 30 days without re-consent?
project: PRJ-0001
status: active            # planned | active | paused | concluded | abandoned
outcome: null             # null | positive | negative | inconclusive | infra
influenced_by: []
created: 2026-11-23
author: agent:claude
protocols: []
materials: []
tags: []
review: pending
---

## Question
Can nitya hold Google and Microsoft 365 access on a headless server for months: do refresh
tokens for a personal, unverified Google Cloud app with Gmail and Calendar scopes, and for
an Entra app registration, stay valid on ghost001 for 30 days without re-consent?

## Hypothesis and predictions
Believed, not yet checked: a Google app in *Testing* status issues refresh tokens that
expire after 7 days; in *Production* but unverified, restricted Gmail scopes show a warning
screen and are capped in users but are allowed for the developer's own account, and refresh
tokens then last until revoked or unused for 6 months. Microsoft refresh tokens for
personal accounts slide for about 90 days; a work tenant may need admin consent.

1. Google, Production and unverified, owner-only: the token obtained on a laptop through
   the desktop flow with loopback redirect and PKCE (as yajna's `sync login` did,
   yajna ADR-0011) and copied to ghost001 keeps working for 30 days.
2. Google in Testing: fails after 7 days, confirming why Testing is unusable.
3. Microsoft: the Entra app's token keeps working for 30 days for a personal account; for
   any work account, record whether admin consent is required.

If 1 fails, nitya needs a verified Google app or IMAP with app passwords for Gmail.

## Plan
1. Create a Google Cloud project for the personal account and an Entra app registration;
   scopes: Calendar read/write, Gmail read and compose (`gmail.modify` only if needed),
   offline access.
   Since 2026-11-23 both come from the `nitya-oauth` terranix stack (ADR-0009): the Entra
   app entirely, the Google project and APIs, with the consent screen and Desktop client
   made by hand from its `google_manual_steps` output.
2. Authorise on the laptop, export the tokens, store them as agenix secrets for ghost001
   (never in this repo), as msgvault's `export-token` does (@msgvault).
3. A small scripted poller on ghost001 that refreshes and lists one calendar and one
   mailbox's latest message id every 6 hours and logs success or the error.
4. Run for 30 days, with a second Google client in Testing as control.

## Success criteria
- For each provider, 30 days of successful refreshes without re-consent, or the exact error
  and date it stopped, and the workaround recorded.
- The enrollment flow for provider accounts in nitya is decided from it.

## Runs
One line per run after its link, what was tried and what it showed; `yajna validate`
rejects a bare link here. Metric runs also land in results.tsv.
- [[RUN-20261126-f98mes]] the 30-day probe on ghost001 every 6 hours: Google, Microsoft
  (rotated and bootstrap chains) and iCloud; running

## Conclusion
Written once, by a human, when status becomes `concluded`.
