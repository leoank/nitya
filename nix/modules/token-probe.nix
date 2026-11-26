# services.nitya-token-probe: EXP-20261123-bhfz49's probe on a NixOS host. Every
# `interval` it refreshes the owner's provider tokens, reads a little with them and
# appends one JSON line per check to /var/lib/nitya-token-probe/probe.jsonl
# (scripts/nitya-token-probe.py). Exported as nixosModules.token-probe.
#
# The token files are passed as systemd credentials: the service runs as a dynamic user
# that cannot read them where they live (agenix's /run/agenix), and sees copies only in
# its credentials directory while it runs. Microsoft's rotated refresh token is kept in
# the state directory (mode 0700, the dynamic user's own).
{ config, lib, pkgs, ... }:
let
  cfg = config.services.nitya-token-probe;
  probe = pkgs.writers.writePython3Bin "nitya-token-probe" {
    flakeIgnore = [ "E501" ];
  } (builtins.readFile ../../scripts/nitya-token-probe.py);
  creds = lib.filterAttrs (_: v: v != null) {
    google-client = cfg.googleClientFile;
    google-token = cfg.googleTokenFile;
    microsoft-token = cfg.microsoftTokenFile;
    icloud = cfg.icloudFile;
  };
  file = lib.mkOption {
    type = lib.types.nullOr lib.types.str;
    default = null;
  };
in
{
  options.services.nitya-token-probe = {
    enable = lib.mkEnableOption "the nitya provider-token probe (EXP-20261123-bhfz49)";
    googleClientFile = file // { description = "The Google Desktop client JSON."; };
    googleTokenFile = file // { description = "nitya-consent's Google token JSON."; };
    microsoftTokenFile = file // { description = "nitya-consent's Microsoft token JSON."; };
    icloudFile = file // { description = ''An iCloud `{"username", "app_password"}` JSON.''; };
    interval = lib.mkOption {
      type = lib.types.str;
      default = "*-*-* 00/6:00:00";
      description = "systemd OnCalendar expression; every 6 hours by default.";
    };
    package = lib.mkOption {
      type = lib.types.package;
      default = probe;
      description = "The probe.";
    };
  };

  config = lib.mkIf cfg.enable {
    systemd.services.nitya-token-probe = {
      description = "nitya provider-token probe (EXP-20261123-bhfz49)";
      wants = [ "network-online.target" ];
      after = [ "network-online.target" ];
      serviceConfig = {
        Type = "oneshot";
        DynamicUser = true;
        StateDirectory = "nitya-token-probe";
        StateDirectoryMode = "0700";
        LoadCredential = lib.mapAttrsToList (n: v: "${n}:${v}") creds;
        ExecStart = lib.concatStringsSep " " ([
          "${cfg.package}/bin/nitya-token-probe check --state /var/lib/nitya-token-probe"
        ] ++ lib.mapAttrsToList (n: _: "--${n} %d/${n}") creds);
        # a failed check is a result, recorded in the log, not a unit failure to retry
        SuccessExitStatus = [ 1 ];
        PrivateTmp = true;
        ProtectSystem = "strict";
        ProtectHome = true;
        NoNewPrivileges = true;
        RestrictAddressFamilies = [ "AF_INET" "AF_INET6" ];
      };
    };
    systemd.timers.nitya-token-probe = {
      wantedBy = [ "timers.target" ];
      timerConfig = {
        OnCalendar = cfg.interval;
        Persistent = true;
        RandomizedDelaySec = "5m";
      };
    };
  };
}
