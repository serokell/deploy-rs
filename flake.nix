# SPDX-FileCopyrightText: 2020 Serokell <https://serokell.io/>
#
# SPDX-License-Identifier: MPL-2.0

{
  inputs = {
    naersk.url = "github:nmattia/naersk/master";
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    utils.url = "github:numtide/flake-utils";
  };

  outputs = { self, nixpkgs, utils, naersk }:
    utils.lib.eachDefaultSystem (system:
      let
        pkgs = import nixpkgs { inherit system; };
        naersk-lib = pkgs.callPackage naersk { };
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
      in
      {
        defaultPackage = naersk-lib.buildPackage ./.;

        defaultApp = {
          type = "app";
          program = "${self.defaultPackage."${system}"}/bin/deploy";
        };

        lib = {
          inherit setActivate;

          checkSchema = deploy: pkgs.runCommandNoCC "jsonschema-deploy-system" { }
            "${pkgs.python3.pkgs.jsonschema}/bin/jsonschema -i ${pkgs.writeText "deploy.json" (builtins.toJSON deploy)} ${./interface/deploy.json} && touch $out";
        };
      });
}
