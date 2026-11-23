# Consent and refresh tokens

Each account nitya syncs needs one sign-in in a browser. `nitya-consent` runs that sign-in
the way a desktop app should (authorization code with PKCE and a loopback redirect,
RFC 8252) and writes the result as JSON:

```json
{
  "provider": "microsoft",
  "client_id": "…",
  "tenant": "common",
  "token_uri": "https://login.microsoftonline.com/common/oauth2/v2.0/token",
  "refresh_token": "…",
  "scope": "…",
  "obtained_at": "2026-11-23T15:00:00Z"
}
```

The refresh token is a secret, so `nitya-consent` never prints it to a terminal: it
writes to `--out`, or to a pipe. Used as agenix's editor, it writes straight into the
temporary file agenix encrypts, so the token is never on disk in the clear anywhere else.
`--out` must come last, because agenix appends the file's path to the editor command.

## A Microsoft account

From your neusis checkout, with the client id from the stack:

```sh
EDITOR="nix run github:leoank/nitya#nitya-consent -- microsoft --client-id <client id> --out" \
  agenix edit modules/secrets/ank/nityaMicrosoftToken.age
```

A browser opens on Microsoft's sign-in page; choose the account, accept the scopes, and
the tab says nitya has the code. `--tenant` narrows who can sign in (`consumers` for
Outlook.com only, or a tenant id); the default, `common`, takes either kind of account.

## A Google account

With the client JSON downloaded in the previous step:

```sh
EDITOR="nix run github:leoank/nitya#nitya-consent -- google --client-json $HOME/Downloads/client_secret_<…>.json --out" \
  agenix edit modules/secrets/ank/nityaGoogleToken.age
```

Then delete the download; the client JSON is already in `nityaGoogleClient.age`.

## On a machine without a browser

`--no-browser` only prints the sign-in URL. The redirect goes to the machine running
`nitya-consent`, so open the URL in a browser on that same machine, or forward the port
the URL names (`ssh -L <port>:localhost:<port>`).

## Getting the tokens to the server

`agenix rekey` encrypts the new secrets for the hosts that use them. Until nitya's server
module exists, nothing on ghost001 reads them; the module will take their paths, and the
machine's configuration will name them the way ghost001's other secrets are named:

```nix
age.secrets.nityaMicrosoftToken.rekeyFile = ../secrets/ank/nityaMicrosoftToken.age;
age.secrets.nityaGoogleToken.rekeyFile = ../secrets/ank/nityaGoogleToken.age;
age.secrets.nityaGoogleClient.rekeyFile = ../secrets/ank/nityaGoogleClient.age;
```

A refresh token lasts until it is revoked or goes unused for a while (six months at
Google, 90 days at Microsoft, each use renewing it); whether it survives
30 days on a headless server for both providers is what
[[EXP-20261123-bhfz49]] measures. To revoke one, remove nitya from your account's
connected apps (Google Account → Security → Third-party connections; Microsoft account →
Privacy → Apps and services), then run the consent again.
