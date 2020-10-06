# SPDX-FileCopyrightText: 2020 Serokell <https://serokell.io/>
#
# SPDX-License-Identifier: MPL-2.0

{
  description = "Deploy GNU hello to localhost";

  inputs.deploy-rs.url = "github:serokell/deploy-rs";

  outputs = { self, nixpkgs, deploy-rs }:
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
          path = deploy-rs.lib.x86_64-linux.setActivate nixpkgs.legacyPackages.x86_64-linux.hello "./bin/hello";
        };
      };

      checks = { "x86_64-linux" = { jsonSchema = deploy-rs.lib.x86_64-linux.checkSchema self.deploy; }; };
    };
}
