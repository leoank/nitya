# Deployment

nitya is deployed with Nix: the server as a NixOS module, the node as a home-manager
module, the cloud pieces as terranix stacks, and its secrets as agenix files in the flake
that configures your machines. None of the modules exist yet; what you can set up today
is the first thing every later step needs, the apps nitya signs in to your providers with.

1. [Provider sign-in apps](provider-apps.md): an app registration at Microsoft, a Google
   Cloud project and a Google OAuth client, from the `nitya-oauth` stack plus four steps in
   Google's console.
2. [Consent and refresh tokens](consent.md): sign each account in once with
   `nitya-consent` and keep the token as an agenix secret for the server.

These pages assume the setup nitya itself is developed with: a machine flake like
[neusis](https://github.com/leoank/neusis), using its terranix stack runner (OpenTofu,
encrypted state committed to git, operator variables in `~/.config/neusis/tf.env`) and
agenix-rekey for secrets. The paths below are neusis's; with another flake, put the same
secrets where yours keeps them.

You need:

- Nix with flakes, and checkouts of nitya and of your machine flake.
- A Google account, for the Google Cloud project and the client.
- A Microsoft Entra tenant of your own, in which you may register applications, with a
  member user that administers it. Not your employer's or university's tenant: its admins
  would own the app, and you lose it with the account. See
  [Provider sign-in apps](provider-apps.md#a-tenant-of-your-own). The app it gets accepts
  personal Outlook.com accounts as well as work and school ones.
