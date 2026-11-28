# Evaluation tests of the server and node modules, run as `checks.modules`. The server is
# evaluated in a real NixOS system; the node against stubs of the home-manager options it
# sets, so the flake needs no home-manager input, on both platforms wherever the check
# runs. A failing test fails evaluation and names itself (as saarthi's module tests do).
{
  pkgs,
  lib,
  nixpkgs,
  self,
}:
let
  fake = name: pkgs.writeShellScriptBin name "exit 0";

  # ---- server ----
  server =
    extra:
    (nixpkgs.lib.nixosSystem {
      system = "x86_64-linux";
      modules = [
        self.nixosModules.server
        {
          system.stateVersion = "25.11";
          boot.isContainer = true;
          services.nitya-server = {
            enable = true;
            package = fake "nitya-server";
          }
          // extra;
        }
      ];
    }).config;
  bare = server { };
  full = server {
    authKeyFile = "/run/agenix/ts";
    delegateKeyFile = "/run/agenix/dk";
    googleClientFile = "/run/agenix/gc";
    hostname = "nitya-test";
    port = 7301;
    pollSeconds = 300;
  };
  svc = c: c.systemd.services.nitya-server;
  exec = c: builtins.unsafeDiscardStringContext (svc c).serviceConfig.ExecStart;
  adminOf = c: lib.findFirst (p: (p.name or "") == "nitya-admin") null c.environment.systemPackages;

  serverTests = {
    test-server-defaults = {
      expr = {
        cmd = lib.hasSuffix "/bin/nitya-server --state-dir /var/lib/nitya --hostname nitya --port 7300 --poll-secs 60" (exec bare);
        noCreds = (svc bare).serviceConfig.LoadCredential == [ ];
        user = (svc bare).serviceConfig.User;
        state = (svc bare).serviceConfig.StateDirectory;
        mode = (svc bare).serviceConfig.StateDirectoryMode;
        systemUser = bare.users.users.nitya.isSystemUser;
        wanted = (svc bare).wantedBy;
        noPorts = bare.networking.firewall.allowedTCPPorts;
        admin = adminOf bare != null;
      };
      expected = {
        cmd = true;
        noCreds = true;
        user = "nitya";
        state = "nitya";
        mode = "0700";
        systemUser = true;
        wanted = [ "multi-user.target" ];
        noPorts = [ ];
        admin = true;
      };
    };
    test-server-secrets-are-credentials = {
      expr = {
        creds = (svc full).serviceConfig.LoadCredential;
        cmd = lib.hasSuffix "--hostname nitya-test --port 7301 --poll-secs 300 --authkey-file %d/authkey --delegate-key %d/delegate-key --google-client %d/google-client" (exec full);
        # The secrets' own paths never reach the command line.
        hidden = !(lib.hasInfix "/run/agenix" (exec full));
      };
      expected = {
        creds = [
          "authkey:/run/agenix/ts"
          "delegate-key:/run/agenix/dk"
          "google-client:/run/agenix/gc"
        ];
        cmd = true;
        hidden = true;
      };
    };
    test-server-admin-runs-as-the-service-user = {
      expr = lib.hasInfix "sudo -u nitya " (adminOf bare).text && lib.hasInfix "--state-dir /var/lib/nitya \"$@\"" (adminOf bare).text;
      expected = true;
    };
  };

  # ---- node ----
  stubHm =
    { lib, ... }:
    let
      anything = lib.mkOption {
        type = lib.types.attrsOf lib.types.anything;
        default = { };
      };
    in
    {
      options = {
        home.packages = lib.mkOption {
          type = lib.types.listOf lib.types.package;
          default = [ ];
        };
        home.homeDirectory = lib.mkOption {
          type = lib.types.str;
          default = "/home/u";
        };
        launchd.agents = anything;
        systemd.user.services = anything;
        systemd.user.timers = anything;
      };
    };
  node =
    system: extra:
    let
      p = import nixpkgs { inherit system; };
    in
    (lib.evalModules {
      modules = [
        stubHm
        self.homeModules.node
        {
          _module.args.pkgs = p;
          services.nitya-node = {
            enable = true;
            package = p.writeShellScriptBin "nitya-node" "exit 0";
            hostname = "nitya-node-rogue";
          }
          // extra;
        }
      ];
    }).config;
  # The scripts' text, read from their derivations without building them.
  wrapperText = c: (builtins.head c.home.packages).text;

  nodeTests = lib.concatMapAttrs (
    tag: system:
    let
      base = node system { };
      keyed = node system { authKeyFile = "/run/agenix/tk"; server = "nitya-test:7301"; };
      off = node system { check.enable = false; };
      isDarwin = lib.hasSuffix "darwin" system;
    in
    {
      "test-node-${tag}-wrapper" = {
        expr = {
          flags = lib.hasInfix "--state-dir /home/u/.local/state/nitya-node --hostname nitya-node-rogue --server nitya:7300 \"$@\"" (wrapperText base);
          authkey = lib.hasInfix "--server nitya-test:7301 --authkey-file /run/agenix/tk \"$@\"" (wrapperText keyed);
          onPath = builtins.length base.home.packages;
        };
        expected = {
          flags = true;
          authkey = true;
          onPath = 1;
        };
      };
      "test-node-${tag}-hourly-check" = {
        expr =
          if isDarwin then
            let
              a = base.launchd.agents.nitya-node-audit;
            in
            {
              args = lib.drop 1 a.config.ProgramArguments;
              every = a.config.StartInterval;
              other = base.systemd.user.timers ? nitya-node-audit;
              off = off.launchd.agents ? nitya-node-audit;
            }
          else
            {
              args = lib.drop 1 (lib.splitString " " base.systemd.user.services.nitya-node-audit.Service.ExecStart);
              every = base.systemd.user.timers.nitya-node-audit.Timer.OnUnitActiveSec;
              other = base.launchd.agents ? nitya-node-audit;
              off = off.systemd.user.timers ? nitya-node-audit;
            };
        expected = {
          args = [
            "audit"
            "verify"
          ];
          every = if isDarwin then 3600 else "3600s";
          other = false;
          off = false;
        };
      };
    }
  ) {
    darwin = "aarch64-darwin";
    linux = "x86_64-linux";
  };

  failures = lib.filterAttrs (_: t: t.expr != t.expected) (serverTests // nodeTests);
in
if failures == { } then
  pkgs.runCommand "nitya-module-tests" { } "touch $out"
else
  throw "module tests failed: ${lib.concatStringsSep ", " (lib.attrNames failures)}\n${
    lib.generators.toPretty { } (lib.mapAttrs (_: t: { inherit (t) expr expected; }) failures)
  }"
