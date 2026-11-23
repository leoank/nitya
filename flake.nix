{
  description = "nitya - one self-hosted server for the day-to-day of life (PRJ-0001)";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-parts.url = "github:hercules-ci/flake-parts";
    flake-parts.inputs.nixpkgs-lib.follows = "nixpkgs";
    # The yajna CLI for the notebook under notes/ (github:leoank/yajna).
    yajna.url = "github:leoank/yajna";
    yajna.inputs.nixpkgs.follows = "nixpkgs";
    # The owner's fleet config: its terranix stack runner (OpenTofu, encrypted state,
    # tf.env) and terranix modules, for nix/infra/ (github:leoank/neusis).
    neusis.url = "github:leoank/neusis";
    terranix.url = "github:terranix/terranix";
    terranix.inputs.nixpkgs.follows = "nixpkgs";
  };

  outputs = inputs@{ flake-parts, ... }:
    flake-parts.lib.mkFlake { inherit inputs; } {
      systems = [ "aarch64-darwin" "x86_64-darwin" "x86_64-linux" "aarch64-linux" ];

      imports = [
        # neusis's terranix bundle declares its input through flake-file, which only
        # neusis's own (generated) flake reads; nitya declares terranix above, so the
        # declaration is accepted here and ignored.
        ({ lib, ... }: {
          options.flake-file = lib.mkOption { type = lib.types.lazyAttrsOf lib.types.raw; default = { }; };
        })
        inputs.neusis.flakeModules.terranix
        ./nix/infra/nitya-oauth.nix
      ];

      perSystem = { config, pkgs, system, lib, ... }:
        let
          yajna = inputs.yajna.packages.${system}.yajna;

          # The cargo workspace; docs/ and notes/ stay out of it.
          src = lib.fileset.toSource {
            root = ./.;
            fileset = lib.fileset.unions [ ./Cargo.toml ./Cargo.lock ./crates ];
          };

          # One binary of the workspace; tests run in checks.workspace.
          bin = name: pkgs.rustPlatform.buildRustPackage {
            pname = name;
            version = "0.1.0";
            inherit src;
            cargoLock.lockFile = ./Cargo.lock;
            cargoBuildFlags = [ "--bin" name ];
            doCheck = false;
            meta.mainProgram = name;
          };
        in
        {
          packages = rec {
            nitya-server = bin "nitya-server";
            nitya-node = bin "nitya-node";
            default = nitya-server;
            # One-time sign-in to Google or Microsoft for a refresh token (ADR-0009);
            # `nix run .#nitya-consent -- --help`.
            nitya-consent = pkgs.writers.writePython3Bin "nitya-consent" {
              flakeIgnore = [ "E501" ];
            } (builtins.readFile ./scripts/nitya-consent.py);
          };

          devShells.default = pkgs.mkShell {
            packages = [
              pkgs.cargo
              pkgs.rustc
              pkgs.clippy
              pkgs.rustfmt
              pkgs.rust-analyzer
              yajna
            ];
            RUST_SRC_PATH = "${pkgs.rustPlatform.rustLibSrc}";
          };

          # The calendar round-trip spike (EXP-20261123-bhfzfs): the default shell plus
          # Radicale as a local CalDAV server.
          devShells.calendar-spike = pkgs.mkShell {
            inputsFrom = [ config.devShells.default ];
            packages = [ pkgs.radicale pkgs.curl ];
          };

          # The cargo workspace, built and tested.
          checks.workspace = pkgs.rustPlatform.buildRustPackage {
            pname = "nitya";
            version = "0.1.0";
            inherit src;
            cargoLock.lockFile = ./Cargo.lock;
          };

          # nitya-consent against a fake provider on loopback, offline.
          checks.nitya-consent = pkgs.runCommand "nitya-consent-test" {
            nativeBuildInputs = [ pkgs.python3 ];
          } ''
            python3 ${./scripts/test-nitya-consent.py} ${config.packages.nitya-consent}/bin/nitya-consent
            touch $out
          '';

          # The notebook passes yajna validate.
          checks.notebook = pkgs.runCommand "nitya-notebook" {
            nativeBuildInputs = [ yajna ];
          } ''
            cd ${lib.fileset.toSource {
              root = ./.;
              # Run pages name their scripts under spikes/; validate checks they exist.
              fileset = lib.fileset.unions [ ./yajna.toml ./notes ./docs ./spikes ];
            }}
            yajna --root . validate
            touch $out
          '';
        };
    };
}
