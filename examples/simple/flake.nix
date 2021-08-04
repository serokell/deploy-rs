# SPDX-FileCopyrightText: 2020 Serokell <https://serokell.io/>
#
# SPDX-License-Identifier: MPL-2.0

{
  description = "Deploy GNU hello to localhost";

  inputs.yeet.url = "github:serokell/yeet";

  outputs = { self, nixpkgs, yeet }: {
    deploy.nodes.example = {
      hostname = "localhost";
      profiles.hello = {
        user = "balsoft";
        path = yeet.lib.x86_64-linux.setActivate nixpkgs.legacyPackages.x86_64-linux.hello "./bin/hello";
      };
    };

    checks = builtins.mapAttrs (system: deployLib: deployLib.deployChecks self.deploy) yeet.lib;
  };
}
