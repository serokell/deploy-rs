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

        lib = {
          setActivate = base: activate: pkgs.symlinkJoin {
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
                destination = "/activate";
              })
            ];
          };

          checkSchema = deploy: pkgs.runCommandNoCC "jsonschema-deploy-system" { }
            "${pkgs.python3.pkgs.jsonschema}/bin/jsonschema -i ${pkgs.writeText "deploy.json" (builtins.toJSON deploy)} ${./interface/deploy.json} && touch $out";
        };
      });
}
