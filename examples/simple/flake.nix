# SPDX-FileCopyrightText: 2020 Serokell <https://serokell.io/>
#
# SPDX-License-Identifier: MPL-2.0

{
  description = "Deploy GNU hello to localhost";

  inputs.deploy-rs.url = "github:serokell/deploy-rs";

  outputs = { self, nixpkgs, deploy-rs }: {
    deploy.nodes.example = {
      hostname = "localhost";
      profiles.hello = {
        user = "balsoft";
        path = deploy-rs.lib.x86_64-linux.setActivate nixpkgs.legacyPackages.x86_64-linux.hello "./bin/hello";
      };
    };

    checks = builtins.mapAttrs (system: deployLib: deployLib.deployChecks self.deploy) deploy-rs.lib;
  };
}
