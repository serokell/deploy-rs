{
  description = "Deploy a full system with hello service as a separate profile";


  outputs = { self, nixpkgs }: {
    nixosConfigurations.example-nixos-system = nixpkgs.lib.nixosSystem {
      system = "x86_64-linux";
      modules = [ ./configuration.nix ];
    };

    nixosConfigurations.bare = nixpkgs.lib.nixosSystem {
      system = "x86_64-linux";
      modules = [ ./bare.nix "${nixpkgs}/nixos/modules/virtualisation/qemu-vm.nix" ];
    };

    # This is the application we actually want to run
    defaultPackage.x86_64-linux = import ./hello.nix nixpkgs;

    deploy.nodes.example = {
      sshOpts = [ "-p" "2221" ];
      hostname = "localhost";
      fastConnection = true;
      profiles = {
        system = {
          sshUser = "admin";
          activate = "$PROFILE/bin/switch-to-configuration switch";
          path = self.nixosConfigurations.example-nixos-system.config.system.build.toplevel;
          user = "root";
        };
        hello = {
          sshUser = "hello";
          activate = "$PROFILE/bin/activate";
          path = self.defaultPackage.x86_64-linux;
          user = "hello";
        };
      };
    };

    checks = builtins.mapAttrs (_: pkgs: {
      jsonschema = pkgs.runCommandNoCC "jsonschema-deploy-system" { }
      "${pkgs.python3.pkgs.jsonschema}/bin/jsonschema -i ${
        pkgs.writeText "deploy.json" (builtins.toJSON self.deploy)
      } ${../../interface/deploy.json} && touch $out";
    }) nixpkgs.legacyPackages;
  };
}
