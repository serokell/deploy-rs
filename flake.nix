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

  outputs = { self, nixpkgs, utils, ... }@inputs:
  {
    overlays.default = final: prev: let
      darwinOptions = final.lib.optionalAttrs final.stdenv.isDarwin {
        buildInputs = with final.darwin.apple_sdk.frameworks; [
          SystemConfiguration
          CoreServices
        ];
      };
    in
    {
      deploy-rs = {
        deploy-rs = final.rustPlatform.buildRustPackage (darwinOptions // {
          pname = "deploy-rs";
          version = "0.1.0";

          src = final.lib.sourceByRegex ./. [
            "Cargo\.lock"
            "Cargo\.toml"
            "src"
            "src/bin"
            ".*\.rs$"
          ];

          runtimeInputs = [ final.pkgs.sops ];

          cargoLock.lockFile = ./Cargo.lock;
	  meta = {
	    description = "A Simple multi-profile Nix-flake deploy tool"; 
            mainProgram = "deploy";
	  };
        });

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
                            elif [[ "''${BOOT:-}" == "1" ]]
                            then
                                ${customSelf.boot or "echo ${final.writeScript "activate" activate}"}
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
                            exec ${final.deploy-rs.deploy-rs}/bin/activate "$@"
                          '';
                          executable = true;
                          destination = "/activate-rs";
                        })
                      ];
                  };
              };

            nixos = base:
              (custom // {
                dryActivate = "$PROFILE/bin/switch-to-configuration dry-activate";
                boot = "$PROFILE/bin/switch-to-configuration boot";
              })
              base.config.system.build.toplevel
              ''
                # work around https://github.com/NixOS/nixpkgs/issues/73404
                cd /tmp

                $PROFILE/bin/switch-to-configuration switch

                # https://github.com/serokell/deploy-rs/issues/31
                ${with base.config.boot.loader;
                final.lib.optionalString systemd-boot.enable
                "sed -i '/^default /d' ${efi.efiSysMountPoint}/loader/loader.conf"}
              '';

            home-manager = base: custom base.activationPackage "$PROFILE/activate";

            # Activation script for 'darwinSystem' from nix-darwin.
            # 'HOME=/var/root' is needed because 'sudo' on darwin doesn't change 'HOME' directory,
            # while 'darwin-rebuild' (which is invoked under the hood) performs some nix-channel
            # checks that rely on 'HOME'. As a result, if 'sshUser' is different from root,
            # deployment may fail without explicit 'HOME' redefinition.
            darwin = base: custom base.config.system.build.toplevel "HOME=/var/root $PROFILE/activate";

            noop = base: custom base ":";
          };

          deployChecks = deploy: builtins.mapAttrs (_: check: check deploy) {
            deploy-schema = deploy: final.runCommand "jsonschema-deploy-system" { } ''
              ${final.check-jsonschema}/bin/check-jsonschema --schemafile ${./interface.json} ${final.writeText "deploy.json" (builtins.toJSON deploy)} && touch $out
            '';

            deploy-activate = deploy:
              let
                profiles = builtins.concatLists (final.lib.mapAttrsToList (nodeName: node: final.lib.mapAttrsToList (profileName: profile: [ (toString profile.path) nodeName profileName ]) node.profiles) deploy.nodes);
              in
              final.runCommand "deploy-rs-check-activate" { } ''
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
    utils.lib.eachSystem (utils.lib.defaultSystems ++ ["aarch64-darwin"]) (system:
      let
        pkgs = import nixpkgs {
          inherit system;
          overlays = [ self.overlays.default ];
        };

        # make a matrix to use in GitHub pipeline
        mkMatrix = name: attrs: {
          include = map (v: { ${name} = v; }) (pkgs.lib.attrNames attrs);
        };
      in
      {
        packages.default = self.packages.${system}.deploy-rs;
        packages.deploy-rs = pkgs.deploy-rs.deploy-rs;

        apps.default = self.apps.${system}.deploy-rs;
        apps.deploy-rs = {
          type = "app";
          program = "${self.packages.${system}.default}/bin/deploy";
        };

        devShells.default = pkgs.mkShell {
          inputsFrom = [ self.packages.${system}.deploy-rs ];
          RUST_SRC_PATH = "${pkgs.rust.packages.stable.rustPlatform.rustLibSrc}";
          buildInputs = with pkgs; [
            nix
            cargo
            rustc
            rust-analyzer
            rustfmt
            clippy
            reuse
            sops
            rust.packages.stable.rustPlatform.rustLibSrc
          ];
        };

        checks = {
          deploy-rs = self.packages.${system}.default.overrideAttrs (super: { doCheck = true; });
        } // (pkgs.lib.optionalAttrs (pkgs.lib.elem system ["x86_64-linux"]) (import ./nix/tests {
          inherit inputs pkgs;
        }));

        inherit (pkgs.deploy-rs) lib;

        check-matrix = mkMatrix "check" self.checks.${system};
      });
}
