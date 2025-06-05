{
  description = "Deploy a full system where the password is supplied via sops";

  inputs = {
    nixpkgs.url = "github:nixos/nixpkgs/nixos-unstable";
    deploy-rs.url = "github:weriomat/deploy-rs/sops";
  };

  outputs =
    {
      self,
      nixpkgs,
      deploy-rs,
      ...
    }:
    let
      system = "x86_64-linux";
      pkgs = import nixpkgs { inherit system; };
    in
    {
      nixosConfigurations = {
        sops = nixpkgs.lib.nixosSystem {
          inherit system;
          modules = [
            ./configuration.nix
            (pkgs.path + "/nixos/modules/virtualisation/qemu-vm.nix")
          ];
        };
        updated = nixpkgs.lib.nixosSystem {
          inherit system;
          modules = [
            (pkgs.path + "/nixos/modules/virtualisation/qemu-vm.nix")
            ./configuration.nix
            ./updated.nix
          ];
        };
      };

      # packages we need to inspect the encrypted files
      devShells.x86_64-linux.default = pkgs.mkShell {
        buildInputs = [
          deploy-rs.packages.default
          pkgs.sops
          pkgs.age
        ];
      };

      deploy.nodes.example = {
        sshOpts = [
          "-p"
          "2221"
        ];
        hostname = "localhost";
        fastConnection = true;
        sudoFile = ./passwords.yaml;

        profiles.system = {
          user = "root";
          sshUser = "deploy";

          # sudo password is gotten via
          sudoSecret = "password/deploy";

          # we setup ssh auth with this key, these will get merged with the settings above
          sshOpts = [
            "-i"
            "./hello_ed25519"
          ];

          path = deploy-rs.lib.x86_64-linux.activate.nixos self.nixosConfigurations.updated; # this is a bit hacky to get a "updated" configuration to deploy
        };
      };

      checks = builtins.mapAttrs (system: deployLib: deployLib.deployChecks self.deploy) deploy-rs.lib;
    };
}
