# SPDX-FileCopyrightText: 2020 Serokell <https://serokell.io/>
# SPDX-FileCopyrightText: 2020 Andreas Fuchs <asf@boinkor.net>
#
# SPDX-License-Identifier: MPL-2.0

{
  description = "A Simple multi-profile Nix-flake deploy tool.";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    utils.url = "github:numtide/flake-utils";
    flake-compat = {
      url = "github:edolstra/flake-compat";
      flake = false;
    };
  };

  outputs = { self, nixpkgs, utils, ... }:
  {
    overlay = final: prev:
    let
      system = final.system;
      isDarwin = final.lib.strings.hasSuffix "-darwin" system;
      darwinOptions = final.lib.optionalAttrs isDarwin {
        nativeBuildInputs = [
          final.darwin.apple_sdk.frameworks.SystemConfiguration
        ];
      };
    in
    {
      deploy-rs = {

        deploy-rs = final.rustPlatform.buildRustPackage (darwinOptions // {
          pname = "deploy-rs";
          version = "0.1.0";

          src = ./.;

          cargoLock.lockFile = ./Cargo.lock;
        }) // { meta.description = "A Simple multi-profile Nix-flake deploy tool"; };

        lib = rec {

          setActivate = builtins.trace
            "deploy-rs#lib.setActivate is deprecated, use activate.noop, activate.nixos or activate.custom instead"
            activate.custom;

          activate = rec {
            custom =
              {
                __functor = customSelf: base: activate:
                  final.buildEnv {
                    name = ("activatable-" + base.name);
                    paths =
                      [
                        base
                        (final.writeTextFile {
                          name = base.name + "-activate-path";
                          text = ''
                            #!${final.runtimeShell}
                            set -euo pipefail

                            if [[ "''${DRY_ACTIVATE:-}" == "1" ]]
                            then
                                ${customSelf.dryActivate or "echo ${final.writeScript "activate" activate}"}
                            else
                                ${activate}
                            fi
                          '';
                          executable = true;
                          destination = "/deploy-rs-activate";
                        })
                        (final.writeTextFile {
                            name = base.name + "-activate-rs";
                            text = ''
                            #!${final.runtimeShell}
                            exec ${self.defaultPackage.${system}}/bin/activate "$@"
                          '';
                          executable = true;
                          destination = "/activate-rs";
                        })
                      ];
                  };
              };

            nixos = base: (custom // { dryActivate = "$PROFILE/bin/switch-to-configuration dry-activate"; }) base.config.system.build.toplevel ''
              # work around https://github.com/NixOS/nixpkgs/issues/73404
              cd /tmp

              $PROFILE/bin/switch-to-configuration switch

              # https://github.com/serokell/deploy-rs/issues/31
              ${with base.config.boot.loader;
              final.lib.optionalString systemd-boot.enable
              "sed -i '/^default /d' ${efi.efiSysMountPoint}/loader/loader.conf"}
            '';

            home-manager = base: custom base.activationPackage "$PROFILE/activate";

            noop = base: custom base ":";
          };

          deployChecks = deploy: builtins.mapAttrs (_: check: check deploy) {
            schema = deploy: final.runCommandNoCC "jsonschema-deploy-system" { } ''
              ${final.python3.pkgs.jsonschema}/bin/jsonschema -i ${final.writeText "deploy.json" (builtins.toJSON deploy)} ${./interface.json} && touch $out
            '';

            activate = deploy:
              let
                profiles = builtins.concatLists (final.lib.mapAttrsToList (nodeName: node: final.lib.mapAttrsToList (profileName: profile: [ (toString profile.path) nodeName profileName ]) node.profiles) deploy.nodes);
              in
              final.runCommandNoCC "deploy-rs-check-activate" { } ''
                for x in ${builtins.concatStringsSep " " (map (p: builtins.concatStringsSep ":" p) profiles)}; do
                  profile_path=$(echo $x | cut -f1 -d:)
                  node_name=$(echo $x | cut -f2 -d:)
                  profile_name=$(echo $x | cut -f3 -d:)

                  test -f "$profile_path/deploy-rs-activate" || (echo "#$node_name.$profile_name is missing the deploy-rs-activate activation script" && exit 1);

                  test -f "$profile_path/activate-rs" || (echo "#$node_name.$profile_name is missing the activate-rs activation script" && exit 1);
                done

                touch $out
              '';
          };
        };
      };
    };
  } //
    utils.lib.eachDefaultSystem (system:
      let
        pkgs = import nixpkgs { inherit system; overlays = [ self.overlay ]; };
      in
      {
        defaultPackage = self.packages."${system}".deploy-rs;
        packages.deploy-rs = pkgs.deploy-rs.deploy-rs;

        defaultApp = self.apps."${system}".deploy-rs;
        apps.deploy-rs = {
          type = "app";
          program = "${self.defaultPackage."${system}"}/bin/deploy";
        };

        devShell = pkgs.mkShell {
          inputsFrom = [ self.packages.${system}.deploy-rs ];
          RUST_SRC_PATH = "${pkgs.rust.packages.stable.rustPlatform.rustLibSrc}";
          buildInputs = with pkgs; [
            nixUnstable
            cargo
            rustc
            rust-analyzer
            rustfmt
            clippy
            reuse
            rust.packages.stable.rustPlatform.rustLibSrc
          ];
        };

        checks = {
          deploy-rs = self.defaultPackage.${system}.overrideAttrs (super: { doCheck = true; });
        };

        lib = pkgs.deploy-rs.lib;
      });
}
