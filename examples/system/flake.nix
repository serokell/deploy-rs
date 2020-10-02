# SPDX-FileCopyrightText: 2020 Serokell <https://serokell.io/>
#
# SPDX-License-Identifier: MPL-2.0

{
  description = "Deploy a full system with hello service as a separate profile";

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
      nixosConfigurations.example-nixos-system = nixpkgs.lib.nixosSystem {
        system = "x86_64-linux";
        modules = [ ./configuration.nix ];
      };

      nixosConfigurations.bare = nixpkgs.lib.nixosSystem {
        system = "x86_64-linux";
        modules =
          [ ./bare.nix "${nixpkgs}/nixos/modules/virtualisation/qemu-vm.nix" ];
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
            path =
              setActivate self.nixosConfigurations.example-nixos-system.config.system.build.toplevel "./bin/switch-to-configuration switch";
            user = "root";
          };
          hello = {
            sshUser = "hello";
            path = setActivate self.defaultPackage.x86_64-linux "./bin/activate";
            user = "hello";
          };
        };
      };

      checks = builtins.mapAttrs
        (_: pkgs: {
          jsonschema = pkgs.runCommandNoCC "jsonschema-deploy-system" { }
            "${pkgs.python3.pkgs.jsonschema}/bin/jsonschema -i ${
          pkgs.writeText "deploy.json" (builtins.toJSON self.deploy)
        } ${../../interface/deploy.json} && touch $out";
        })
        nixpkgs.legacyPackages;
    };
}
