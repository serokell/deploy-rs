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
      system = final.stdenv.hostPlatform.system;
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
                  base.overrideAttrs (oldAttrs: {
                    name = "activatable-${base.name}";
                    buildCommand = ''
                      set -euo pipefail
                      ${nixpkgs.lib.concatStringsSep "\n" (map (outputName:
                        let
                          activatePath = final.writeShellScript (base.name + "-activate-path") ''
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

                          activateRs = final.writeShellScript (base.name + "-activate-rs") ''
                            exec ${self.packages.${system}.default}/bin/activate "$@"
                          '';
                        in (''
                          ${final.coreutils}/bin/mkdir "''$${outputName}"

                          echo "Linking activation components in ${outputName}"
                          ${final.coreutils}/bin/ln -s "${activatePath}" "''$${outputName}/deploy-rs-activate"
                          ${final.coreutils}/bin/ln -s "${activateRs}" "''$${outputName}/activate-rs"

                          echo "Linking output contents of ${outputName}"
                          ${final.findutils}/bin/find "${base.${outputName}}" -maxdepth 1 | while read -r file; do
                            ${final.coreutils}/bin/ln -s "$file" "''$${outputName}/$(${final.coreutils}/bin/basename "$file")"
                          done
                        '' + nixpkgs.lib.optionalString
                          (outputName == "out") ''
                            # Workaround for https://github.com/serokell/deploy-rs/issues/185
                            if [ -x "${base.${outputName}}/prepare-root" ]; then
                              echo "Copying prepare-root"
                              rm "$out/prepare-root" || :
                              cp "${base.${outputName}}/prepare-root" "$out/prepare-root"
                            fi
                          '')) (base.outputs or [ "out" ]))}
                    '';
                  });
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

            noop = base: custom base ":";
          };

          deployChecks = deploy: builtins.mapAttrs (_: check: check deploy) {
            deploy-schema = deploy: final.runCommand "jsonschema-deploy-system" { } ''
              ${final.python3.pkgs.jsonschema}/bin/jsonschema -i ${final.writeText "deploy.json" (builtins.toJSON deploy)} ${./interface.json} && touch $out
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
        pkgs = import nixpkgs { inherit system; overlays = [ self.overlay ]; };
      in
      {
        defaultPackage = self.packages."${system}".deploy-rs;
        packages.default = self.packages."${system}".deploy-rs;
        packages.deploy-rs = pkgs.deploy-rs.deploy-rs;

        defaultApp = self.apps."${system}".deploy-rs;
        apps.default = self.apps."${system}".deploy-rs;
        apps.deploy-rs = {
          type = "app";
          program = "${self.packages."${system}".default}/bin/deploy";
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
          deploy-rs = self.packages.${system}.default.overrideAttrs (super: { doCheck = true; });
        };

        lib = pkgs.deploy-rs.lib;
      });
}
