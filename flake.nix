# SPDX-FileCopyrightText: 2020 Serokell <https://serokell.io/>
# SPDX-FileCopyrightText: 2020 Andreas Fuchs <asf@boinkor.net>
#
# SPDX-License-Identifier: MPL-2.0

{
  description = "A Simple multi-profile Nix-flake deploy tool.";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    naersk = {
      url = "github:nmattia/naersk/master";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    utils.url = "github:numtide/flake-utils";
    flake-compat = {
      url = "github:edolstra/flake-compat";
      flake = false;
    };
  };

  outputs = { self, nixpkgs, utils, naersk, ... }:
    utils.lib.eachDefaultSystem (system:
      let
        pkgs = import nixpkgs { inherit system; };
        naersk-lib = pkgs.callPackage naersk { };
        isDarwin = pkgs.lib.strings.hasSuffix "-darwin" system;
        darwinOptions = pkgs.lib.optionalAttrs isDarwin {
          nativeBuildInputs = [
            pkgs.darwin.apple_sdk.frameworks.SystemConfiguration
          ];
        };
      in
      {
        defaultPackage = self.packages."${system}".deploy-rs;
        packages.deploy-rs = naersk-lib.buildPackage (darwinOptions // {
          root = ./.;
        });

        defaultApp = self.apps."${system}".deploy-rs;
        apps.deploy-rs = {
          type = "app";
          program = "${self.defaultPackage."${system}"}/bin/deploy";
        };

        devShell = pkgs.mkShell {
          inputsFrom = [ self.packages.${system}.deploy-rs ];
          buildInputs = [ pkgs.nixUnstable ];
        };

        lib = rec {

          setActivate = builtins.trace
            "deploy-rs#lib.setActivate is deprecated, use activate.noop, activate.nixos or activate.custom instead"
            activate.custom;

          activate = rec {
            custom = base: activate: pkgs.buildEnv {
              name = ("activatable-" + base.name);
              paths = [
                base
                (pkgs.writeTextFile {
                  name = base.name + "-activate-path";
                  text = ''
                    #!${pkgs.runtimeShell}
                    set -euo pipefail

                    ${activate}
                  '';
                  executable = true;
                  destination = "/deploy-rs-activate";
                })
                (pkgs.writeTextFile {
                  name = base.name + "-activate-rs";
                  text = ''
                    #!${pkgs.runtimeShell}
                    exec ${self.defaultPackage."${system}"}/bin/activate "$@"
                  '';
                  executable = true;
                  destination = "/activate-rs";
                })
              ];
            };

            nixos = base: custom base.config.system.build.toplevel ''
              $PROFILE/bin/switch-to-configuration switch

              # https://github.com/serokell/deploy-rs/issues/31
              ${with base.config.boot.loader;
              pkgs.lib.optionalString systemd-boot.enable
              "sed -i '/^default /d' ${efi.efiSysMountPoint}/loader/loader.conf"}
            '';

            noop = base: custom base ":";
          };

          deployChecks = deploy: builtins.mapAttrs (_: check: check deploy) {
            schema = deploy: pkgs.runCommandNoCC "jsonschema-deploy-system" { } ''
              ${pkgs.python3.pkgs.jsonschema}/bin/jsonschema -i ${pkgs.writeText "deploy.json" (builtins.toJSON deploy)} ${./interface.json} && touch $out
            '';

            activate = deploy:
              let
                profiles = builtins.concatLists (pkgs.lib.mapAttrsToList (nodeName: node: pkgs.lib.mapAttrsToList (profileName: profile: [ (toString profile.path) nodeName profileName ]) node.profiles) deploy.nodes);
              in
              pkgs.runCommandNoCC "deploy-rs-check-activate" { } ''
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
      });
}
