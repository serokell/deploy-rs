# SPDX-FileCopyrightText: 2020 Serokell <https://serokell.io/>
#
# SPDX-License-Identifier: MPL-2.0

{
  description = "Deploy GNU hello to localhost";

  outputs = { self, nixpkgs }:
    let
      setActivate = base: activate: nixpkgs.legacyPackages.x86_64-linux.symlinkJoin {
        name = ("activatable-" + base.name);
        paths = [
          base
          (nixpkgs.legacyPackages.x86_64-linux.writeTextFile {
            name = base.name + "-activate-path";
            text = ''
              #!${nixpkgs.legacyPackages.x86_64-linux.runtimeShell}
              ${activate}
            '';
            executable = true;
            destination = "/activate";
          })
        ];
      };
    in
    {

      deploy.nodes.example = {
        hostname = "localhost";
        profiles.hello = {
          user = "balsoft";
          path = setActivate nixpkgs.legacyPackages.x86_64-linux.hello "./bin/hello";
        };
      };
      checks = builtins.mapAttrs
        (_: pkgs: {
          jsonschema = pkgs.runCommandNoCC "jsonschema-deploy-simple" { }
            "${pkgs.python3.pkgs.jsonschema}/bin/jsonschema -i ${
          pkgs.writeText "deploy.json" (builtins.toJSON self.deploy)
        } ${../../interface/deploy.json} && touch $out";
        })
        nixpkgs.legacyPackages;
    };
}
