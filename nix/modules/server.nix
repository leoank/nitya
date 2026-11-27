# services.nitya-server: nitya's server on a NixOS host (ADR-0001, ADR-0016), exported
# as nixosModules.server.
#
# It joins the tailnet as its own identity and serves its API there only; nothing listens
# on the host's interfaces. Its state (tailnet identity, store, CA and audit keys) is in
# /var/lib/nitya, readable only by the `nitya` user. The tailnet auth key and the
# sync-delegate key are passed as systemd credentials: agenix secrets the service user
# cannot read where they live, copied for the unit while it runs.
#
# `nitya-admin` runs the admin commands (enroll-code, devices, revoke) against that state
# as the `nitya` user, through sudo.
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
  cfg = config.services.nitya-server;
  dir = "/var/lib/nitya";
  exe = lib.getExe cfg.package;
  creds = lib.filter (c: c.file != null) [
    { name = "authkey"; flag = "--authkey-file"; file = cfg.authKeyFile; }
    { name = "delegate-key"; flag = "--delegate-key"; file = cfg.delegateKeyFile; }
  ];
  admin = pkgs.writeShellScriptBin "nitya-admin" ''
    # nitya's admin commands against the server's state, as its user.
    exec ${config.security.wrapperDir}/sudo -u ${cfg.user} ${exe} --state-dir ${dir} "$@"
  '';
in
{
  options.services.nitya-server = {
    enable = lib.mkEnableOption "the nitya server";

    package = lib.mkOption {
      type = lib.types.package;
      default = self.packages.${pkgs.stdenv.hostPlatform.system}.nitya-server;
      defaultText = lib.literalExpression "nitya.packages.\${system}.nitya-server";
      description = "The package providing `nitya-server`.";
    };

    hostname = lib.mkOption {
      type = lib.types.str;
      default = "nitya";
      description = "The server's machine name on the tailnet; nodes reach it as `<hostname>:<port>`.";
    };

    port = lib.mkOption {
      type = lib.types.port;
      default = 7300;
      description = "The port the server serves on its tailnet identity.";
    };

    authKeyFile = lib.mkOption {
      type = lib.types.nullOr lib.types.str;
      default = null;
      example = "/run/agenix/nityaServerTsAuthKey";
      description = ''
        A tailnet auth key, for the first join and when the tailnet has logged the
        identity out. Null once the identity in /var/lib/nitya/tailnet is enough.
      '';
    };

    delegateKeyFile = lib.mkOption {
      type = lib.types.nullOr lib.types.str;
      default = null;
      example = "/run/agenix/nityaDelegateKey";
      description = ''
        The sync-delegate key, an age identity as `age-keygen` writes it (ADR-0013,
        ADR-0016). Null: the server makes one in its state directory.
      '';
    };

    user = lib.mkOption {
      type = lib.types.str;
      default = "nitya";
      description = "The service user, owner of /var/lib/nitya.";
    };

    extraArgs = lib.mkOption {
      type = lib.types.listOf lib.types.str;
      default = [ ];
      description = "More arguments for `nitya-server`.";
    };
  };

  config = lib.mkIf cfg.enable {
    users.users.${cfg.user} = {
      isSystemUser = true;
      group = cfg.user;
      home = dir;
      description = "nitya server";
    };
    users.groups.${cfg.user} = { };

    environment.systemPackages = [ admin ];

    systemd.services.nitya-server = {
      description = "nitya server: store, sync, policy, audit, device CA";
      wantedBy = [ "multi-user.target" ];
      after = [ "network-online.target" ];
      wants = [ "network-online.target" ];
      environment.HOME = dir;
      serviceConfig = {
        ExecStart = lib.concatStringsSep " " (
          [
            exe
            "--state-dir"
            dir
            "--hostname"
            cfg.hostname
            "--port"
            (toString cfg.port)
          ]
          ++ lib.concatMap (c: [ c.flag "%d/${c.name}" ]) creds
          ++ map lib.escapeShellArg cfg.extraArgs
        );
        LoadCredential = map (c: "${c.name}:${c.file}") creds;
        User = cfg.user;
        Group = cfg.user;
        StateDirectory = "nitya";
        StateDirectoryMode = "0700";
        UMask = "0077";
        Restart = "on-failure";
        RestartSec = 5;
        # An embedded tailnet: TCP and UDP out, unix socket pairs, netlink to watch the
        # host's interfaces; nothing else.
        NoNewPrivileges = true;
        ProtectSystem = "strict";
        ProtectHome = true;
        PrivateTmp = true;
        PrivateDevices = true;
        ProtectKernelTunables = true;
        ProtectKernelModules = true;
        ProtectControlGroups = true;
        RestrictAddressFamilies = [
          "AF_INET"
          "AF_INET6"
          "AF_UNIX"
          "AF_NETLINK"
        ];
        RestrictNamespaces = true;
        LockPersonality = true;
      };
    };
  };
}
