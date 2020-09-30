# SPDX-FileCopyrightText: 2020 Serokell <https://serokell.io/>
#
# SPDX-License-Identifier: MPL-2.0

{
  description = "Deploy GNU hello to localhost";

  outputs = { self, nixpkgs }: {
    deploy.nodes.example = {
      hostname = "localhost";
      profiles.hello = {
        user = "balsoft";
        path = nixpkgs.legacyPackages.x86_64-linux.hello;
        # Just to test that it's working
        activate = "$PROFILE/bin/hello";
      };
    };
    checks = builtins.mapAttrs (_: pkgs: {
      jsonschema = pkgs.runCommandNoCC "jsonschema-deploy-simple" { }
        "${pkgs.python3.pkgs.jsonschema}/bin/jsonschema -i ${
          pkgs.writeText "deploy.json" (builtins.toJSON self.deploy)
        } ${../../interface/deploy.json} && touch $out";
    }) nixpkgs.legacyPackages;
  };
}
