# Stack `nitya-oauth`: the OAuth apps nitya signs in to providers with, for
# EXP-20261123-bhfzfs and EXP-20261123-bhfz49 and later the channel drivers.
#
# - Microsoft: an Entra app registration, a public client (no secret) that
#   personal and work accounts can consent to, with the delegated Graph scopes
#   nitya needs. Fully declared here.
# - Google: the project and its Calendar and Gmail APIs. Google has no API for
#   the consent screen or a Desktop OAuth client (the IAP OAuth Admin API, the
#   only one, shut down on 2026-03-19), so those two are made by hand in the
#   console; the `google_manual_steps` output says how.
#
# Runs with neusis's stack defaults (flakeModules.terranix): from the repo
# root, encrypted state committed under infra/nitya-oauth, TF_VAR_* from
# ~/.config/neusis/tf.env (state passphrase, google_project_id,
# azure_tenant_id). Credentials come from the operator's CLI logins:
#
#   nix develop .#nitya-oauth-login -c gcloud auth application-default login
#   nix develop .#nitya-oauth-login -c az login --allow-no-subscriptions
#   nix run .#nitya-oauth.plan / .apply
#   nix run .#nitya-oauth.terraform -- output microsoft_client_id
#
# Refresh tokens from the one-time consent are agenix secrets in neusis,
# rekeyed for ghost001; nothing secret is an output of this stack.
{ inputs, ... }:
let
  graph = scope: "\${data.azuread_service_principal.msgraph.oauth2_permission_scope_ids[\"${scope}\"]}";
  # Delegated Graph scopes: calendars for phase 2, mail read and drafts for
  # phase 3 (ADR-0004: agents draft, nitya never sends unasked), refresh tokens.
  graphScopes = [ "Calendars.ReadWrite" "Mail.ReadWrite" "User.Read" "offline_access" ];

  stack = {
    variable.google_project_id = {
      type = "string";
      description = "Id of the Google Cloud project nitya's OAuth client lives in (globally unique).";
    };
    variable.azure_tenant_id = {
      type = "string";
      description = "Entra tenant the app registration is created in.";
    };

    terraform.required_providers = {
      google.source = "hashicorp/google";
      azuread.source = "hashicorp/azuread";
    };
    provider.google.project = "\${var.google_project_id}";
    provider.azuread.tenant_id = "\${var.azure_tenant_id}";

    # Google: a project under the personal account (no organization), and
    # the APIs nitya calls. Billing is not needed for either API.
    resource.google_project.nitya = {
      name = "nitya";
      project_id = "\${var.google_project_id}";
      deletion_policy = "PREVENT";
    };
    resource.google_project_service = {
      calendar = {
        project = "\${google_project.nitya.project_id}";
        service = "calendar-json.googleapis.com";
        disable_on_destroy = false;
      };
      gmail = {
        project = "\${google_project.nitya.project_id}";
        service = "gmail.googleapis.com";
        disable_on_destroy = false;
      };
    };

    output.google_manual_steps.value = ''
      In https://console.cloud.google.com/auth/overview?project=''${google_project.nitya.project_id}:
      1. Branding: app name "nitya", your address as support and developer contact.
      2. Audience: External, then "Publish app" (In production). Testing status
         expires refresh tokens after 7 days (EXP-20261123-bhfz49).
      3. Data access: add the scopes .../auth/calendar and .../auth/gmail.modify.
      4. Clients: Create client, type "Desktop app", name "nitya"; download the JSON
         and store it with agenix in neusis (never in this repo).
    '';

    # Microsoft: Graph and the ids of its delegated scopes, looked up by name.
    data.azuread_application_published_app_ids.well_known = { };
    data.azuread_service_principal.msgraph = {
      client_id = "\${data.azuread_application_published_app_ids.well_known.result[\"MicrosoftGraph\"]}";
    };

    resource.azuread_application.nitya = {
      display_name = "nitya";
      # Outlook.com and Microsoft 365 accounts alike; personal accounts need
      # v2 access tokens.
      sign_in_audience = "AzureADandPersonalMicrosoftAccount";
      api.requested_access_token_version = 2;
      # A public client: the loopback redirect of a desktop flow with PKCE,
      # as yajna's Google sign-in does; there is no client secret to keep.
      fallback_public_client_enabled = true;
      public_client.redirect_uris = [ "http://localhost/" ];
      required_resource_access = [
        {
          resource_app_id = "\${data.azuread_application_published_app_ids.well_known.result[\"MicrosoftGraph\"]}";
          resource_access = map (s: { id = graph s; type = "Scope"; }) graphScopes;
        }
      ];
    };

    output.microsoft_client_id.value = "\${azuread_application.nitya.client_id}";
    output.google_project_id.value = "\${google_project.nitya.project_id}";
  };
in
{
  perSystem =
    { pkgs, ... }:
    let
      tofu = pkgs.opentofu.withPlugins (p: [
        p.hashicorp_google
        p.hashicorp_azuread
      ]);
      modules = [
        inputs.neusis.terranixModules.state-encryption
        stack
      ];
    in
    {
      terranix.terranixConfigurations.nitya-oauth = {
        inherit modules;
        terraformWrapper.package = tofu;
        # The azuread provider signs in by calling `az`.
        terraformWrapper.extraRuntimeInputs = [ pkgs.azure-cli ];
      };

      # The CLIs to sign in with once, before plan and apply; the google provider
      # reads gcloud's application-default credentials file.
      devShells.nitya-oauth-login = pkgs.mkShell {
        packages = [ pkgs.google-cloud-sdk pkgs.azure-cli ];
      };

      # The stack is valid OpenTofu with these providers (no credentials needed).
      checks.tofu-validate-nitya-oauth = pkgs.runCommand "tofu-validate-nitya-oauth"
        { nativeBuildInputs = [ tofu ]; }
        ''
          export HOME=$TMPDIR
          mkdir work && cd work
          ln -s ${inputs.terranix.lib.terranixConfiguration { inherit pkgs modules; }} config.tf.json
          tofu init -backend=false -input=false >/dev/null
          tofu validate
          touch $out
        '';
    };
}
