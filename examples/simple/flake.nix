{
  description = "Deploy GNU hello to localhost";

  outputs = { self, nixpkgs }: {
    deploy.nodes.example = {
      hostname = "localhost";
      profiles.hello = {
        user = "test_deploy";
        path = nixpkgs.legacyPackages.x86_64-linux.hello;
        # Just to test that it's working
        activate = "$PROFILE/bin/hello";
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
