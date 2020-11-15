# SPDX-FileCopyrightText: 2020 Serokell <https://serokell.io/>
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
      in
      {
        defaultPackage = self.packages."${system}".deploy-rs;
        packages.deploy-rs = naersk-lib.buildPackage ./.;

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
          setActivate = base: activate: pkgs.buildEnv {
            name = ("activatable-" + base.name);
            paths = [
              base
              (pkgs.writeTextFile {
                name = base.name + "-activate-path";
                text = ''
                  #!${pkgs.runtimeShell}
                  ${activate}
                '';
                executable = true;
                destination = "/deploy-rs-activate";
              })
            ];
          };

          nixosActivate = base: setActivate base "$PROFILE/bin/switch-to-configuration switch";

          noopActivate = base: setActivate base ":";

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

                  test -f "$profile_path/deploy-rs-activate" || (echo "#$node_name.$profile_name is missing an activation script" && exit 1);
                done

                touch $out
              '';
          };
        };
      });
}
