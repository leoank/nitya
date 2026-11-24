# Provider sign-in apps

nitya signs in to Google and Microsoft as an OAuth *desktop* app: you consent once in a
browser, and the server keeps the refresh token. The apps are declared in the
`nitya-oauth` terranix stack (`nix/infra/nitya-oauth.nix`, [ADR-0009](../design/decisions/ADR-0009-provider-oauth-apps-are-a-terranix-stack-here-their-secrets-agenix-in-neusis.md)):

| | Declared by the stack | By hand |
|---|---|---|
| Microsoft | the app registration: personal and work accounts, a public client with a `http://localhost` redirect, the delegated Graph scopes `Calendars.ReadWrite`, `Mail.ReadWrite`, `User.Read`, `offline_access` | nothing |
| Google | the project, the Calendar and Gmail APIs | the consent screen and the Desktop client |

Google has no API for creating a consent screen or a Desktop OAuth client (the only
client API it had, the IAP OAuth Admin API, made IAP-only clients and was shut down on
2026-03-19), so those two stay steps in the console. The Microsoft app has no client
secret at all; the Google client has one, which Google itself does not treat as
confidential for desktop apps.

## A tenant of your own

The Microsoft app lives in an Entra tenant, and the account that runs the stack must be a
member of it. Two accounts that look usable are not:

- **A work or school account**, such as the university account an Azure for Students
  offer is tied to, sees only its organisation's tenant. Don't create nitya's app there.
- **A personal account (Outlook.com, Hotmail) with no directory** has no tenant at all.
  `az login` still succeeds, in Microsoft's own *Microsoft Services* tenant, where Graph
  answers every call with `User was not found`; the azuread provider reports that as
  *the context used must have a deadline attached*.

So make one: in the [Entra admin center](https://entra.microsoft.com), **Manage tenants →
Create → Microsoft Entra ID** (free, no subscription), or sign the personal account up
for an Azure free account, which creates a *Default Directory*. In the new tenant create
a user, such as `admin@<name>.onmicrosoft.com`, with the **Global Administrator** role,
and run the stack as that user. It belongs to the tenant, so Graph knows it, and it does
not depend on another account that may close.

## 1. Add the stack's variables to `tf.env`

The stack reads two values from your operator environment, next to the state passphrase
every neusis stack already uses. In neusis, edit the secret that becomes
`~/.config/neusis/tf.env` and add:

```sh
agenix edit modules/secrets/ank/ociTfEnv.age
```

```sh
TF_VAR_google_project_id=nitya-<something unique>   # Google project ids are global
TF_VAR_azure_tenant_id=<your own tenant id>
```

Then `agenix rekey` and switch your home configuration so the file is updated.

The Google project may already exist, for example made in the console: give its id here
and import it in step 3 instead of letting the stack create it.

## 2. Sign in to both clouds

The providers use your own logins: Google's application-default credentials and the
Azure CLI, as the tenant's admin user. From the nitya checkout:

```sh
nix develop .#nitya-oauth-login -c gcloud auth application-default login
nix develop .#nitya-oauth-login -c env AZURE_CORE_LOGIN_EXPERIENCE_V2=off \
  az login --allow-no-subscriptions --tenant <tenant id>
```

`AZURE_CORE_LOGIN_EXPERIENCE_V2=off` skips the CLI's tenant and subscription picker,
which crashes (`'NoneType' object has no attribute 'get'`) when the login has no
subscription, as a tenant made for nitya has none. Check the login before planning:

```sh
# the tenants this login can see; yours, not an empty list
nix develop .#nitya-oauth-login -c az rest --method get \
  --url "https://management.azure.com/tenants?api-version=2022-12-01" \
  --query "value[].{name:displayName, domain:defaultDomain, id:tenantId}" -o table
# the admin user, not "User was not found"
nix develop .#nitya-oauth-login -c az ad signed-in-user show --query userPrincipalName
```

(`az account tenant list` needs an extension that the CLI from Nix cannot install.)

Once the Google project exists, make it the quota project of your credentials; without
one, API calls are counted against gcloud's shared project, which warns at login and
refuses requests once it is over its limit:

```sh
nix develop .#nitya-oauth-login -c gcloud auth application-default set-quota-project <project id>
```

If the stack is to create the project, run this after the apply, and apply again if
enabling the APIs failed with *requires a quota project*.

## 3. Plan and apply

Stacks run from the repository root. If you made the Google project yourself, import it
first; creating it again fails with `409 alreadyExists`:

```sh
nix run .#nitya-oauth.terraform -- import google_project.nitya <project id>   # only then
nix run .#nitya-oauth.plan
nix run .#nitya-oauth.apply
```

The plan creates the Entra app, the Google project (unless imported) and its Calendar and
Gmail APIs; the app's service principal appears in your tenant at the first consent. The
state is encrypted with your state passphrase and written to
`infra/nitya-oauth/terraform.tfstate`; commit it, as neusis commits its stacks' state.
`nix run .#nitya-oauth.plan -- -detailed-exitcode` later tells you whether anything has
drifted.

## 4. Read the outputs

```sh
nix run .#nitya-oauth.terraform -- output -raw microsoft_client_id
nix run .#nitya-oauth.terraform -- output -raw google_manual_steps
```

The Microsoft client id is not a secret; you pass it to `nitya-consent`.

## 5. Finish Google's side in the console

`google_manual_steps` prints these with your project's link. In the Google Auth Platform
of the project:

1. **Branding**: app name `nitya`, your address as support and developer contact.
2. **Audience**: External, then **Publish app** so its status is *In production*. A client
   left in *Testing* gets refresh tokens that expire after seven days.
3. **Data access**: add the scopes `https://www.googleapis.com/auth/calendar` and
   `https://www.googleapis.com/auth/gmail.modify`.
4. **Clients**: create a client of type **Desktop app** named `nitya` and download its
   JSON.

Because the app is unverified, Google shows a warning on the consent page; as the
project's owner signing in to your own account, you can continue past it.

Keep the downloaded JSON as a secret in neusis, then delete the download once the
[consent](consent.md) step has used it:

```sh
agenix edit -i ~/Downloads/client_secret_<…>.json modules/secrets/ank/nityaGoogleClient.age
```
