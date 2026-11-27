# services.nitya-node: nitya's node for one user on one machine (ADR-0001, ADR-0016), as
# a home-manager module, exported as homeModules.node.
#
# Puts `nitya-node` on PATH with this machine's state directory, tailnet hostname, server
# and auth key already given, so `nitya-node enroll CODE`, `nitya-node keys status` and
# the rest need nothing else. Every hour a user service (systemd timer on Linux, launchd
# agent on Darwin) runs `nitya-node audit verify`, which checks the account's audit log
# against this device's checkpoint (ADR-0015) and renews its certificate; until the node
# is enrolled it fails and says so in its log.
#
# `self` is this flake, for the default package.
self:
{
  config,
  lib,
  pkgs,
  ...
}:
let
  cfg = config.services.nitya-node;
  isDarwin = pkgs.stdenv.hostPlatform.isDarwin;
  args = [
    (lib.getExe cfg.package)
    "--state-dir"
    cfg.stateDir
    "--hostname"
    cfg.hostname
    "--server"
    cfg.server
  ]
  ++ lib.optionals (cfg.authKeyFile != null) [
    "--authkey-file"
    cfg.authKeyFile
  ];
  wrapper = pkgs.writeShellScriptBin "nitya-node" ''
    # nitya-node with this machine's settings (services.nitya-node).
    exec ${lib.escapeShellArgs args} "$@"
  '';
  verify = [
    (lib.getExe wrapper)
    "audit"
    "verify"
  ];
in
{
  options.services.nitya-node = {
    enable = lib.mkEnableOption "the nitya node";

    package = lib.mkOption {
      type = lib.types.package;
      default = self.packages.${pkgs.stdenv.hostPlatform.system}.nitya-node;
      defaultText = lib.literalExpression "nitya.packages.\${system}.nitya-node";
      description = "The package providing the real `nitya-node`, which the wrapper calls.";
    };

    hostname = lib.mkOption {
      type = lib.types.str;
      example = "nitya-node-rogue";
      description = "This node's machine name on the tailnet, one per machine.";
    };

    server = lib.mkOption {
      type = lib.types.str;
      default = "nitya:7300";
      description = "The server on the tailnet, `host:port`.";
    };

    stateDir = lib.mkOption {
      type = lib.types.str;
      default = "${config.home.homeDirectory}/.local/state/nitya-node";
      defaultText = lib.literalExpression ''"''${config.home.homeDirectory}/.local/state/nitya-node"'';
      description = ''
        The node's tailnet identity, device and wrap keys, certificate and audit checkpoint.
      '';
    };

    authKeyFile = lib.mkOption {
      type = lib.types.nullOr lib.types.str;
      default = null;
      example = "/run/agenix/nityaNodeTsAuthKey";
      description = ''
        A tailnet auth key, for the first join and when the tailnet has logged the
        identity out. On Darwin give a fixed path: launchd does not expand the per-user
        temporary directory agenix defaults to.
      '';
    };

    check = {
      enable = lib.mkOption {
        type = lib.types.bool;
        default = true;
        description = "Check the audit log (and renew the certificate) every hour.";
      };
      intervalSeconds = lib.mkOption {
        type = lib.types.ints.positive;
        default = 3600;
        description = "How often the check runs.";
      };
    };
  };

  config = lib.mkIf cfg.enable (
    lib.mkMerge [
      { home.packages = [ wrapper ]; }

      (lib.mkIf (cfg.check.enable && !isDarwin) {
        systemd.user.services.nitya-node-audit = {
          Unit.Description = "nitya node: check the audit log against this device's checkpoint";
          Service = {
            Type = "oneshot";
            ExecStart = lib.escapeShellArgs verify;
          };
        };
        systemd.user.timers.nitya-node-audit = {
          Unit.Description = "nitya node: hourly audit log check";
          Timer = {
            OnActiveSec = "5m";
            OnUnitActiveSec = "${toString cfg.check.intervalSeconds}s";
            RandomizedDelaySec = "5m";
          };
          Install.WantedBy = [ "timers.target" ];
        };
      })

      (lib.mkIf (cfg.check.enable && isDarwin) {
        launchd.agents.nitya-node-audit = {
          enable = true;
          config = {
            ProgramArguments = verify;
            StartInterval = cfg.check.intervalSeconds;
            RunAtLoad = true;
            ProcessType = "Background";
            StandardOutPath = "${config.home.homeDirectory}/Library/Logs/nitya-node-audit.log";
            StandardErrorPath = "${config.home.homeDirectory}/Library/Logs/nitya-node-audit.log";
          };
        };
      })
    ]
  );
}
