# SPDX-FileCopyrightText: 2024 Serokell <https://serokell.io/>
# SPDX-FileCopyrightText: 2024 Sefa Eyeoglu <contact@scrumplex.net>
#
# SPDX-License-Identifier: MPL-2.0

{
  inputs = {
    # real inputs are substituted in ./default.nix
    ##inputs##
  };

  outputs = inputs@{ deploy-rs, flake-parts, nixpkgs, self, ... }:
    flake-parts.lib.mkFlake { inherit inputs; } {
      systems = nixpkgs.lib.systems.flakeExposed;
      imports = [ deploy-rs.flakeModule ];
      flake =
        let
          system = "x86_64-linux";
          pkgs = nixpkgs.legacyPackages.${system};
          user = "deploy";
        in
        {
          nixosConfigurations.server = nixpkgs.lib.nixosSystem {
            inherit system pkgs;
            specialArgs = { inherit inputs; };
            modules = [
              ./server.nix
              ./common.nix
              # Import the base config used by nixos tests
              (pkgs.path + "/nixos/lib/testing/nixos-test-base.nix")
              # Deployment breaks the network settings, so we need to restore them
              (pkgs.lib.importJSON ./network.json)
              # Deploy packages
              { environment.systemPackages = [ pkgs.figlet pkgs.hello ]; }
            ];
          };

          deploy.nodes = {
            server = {
              hostname = "server";
              sshUser = "root";
              sshOpts = [
                "-o"
                "StrictHostKeyChecking=no"
                "-o"
                "StrictHostKeyChecking=no"
              ];
              profiles.system.path = deploy-rs.lib."${system}".activate.nixos
                self.nixosConfigurations.server;
            };
            server-override = {
              hostname = "override";
              sshUser = "override";
              user = "override";
              sudo = "override";
              sshOpts = [ ];
              confirmTimeout = 0;
              activationTimeout = 0;
              profiles.system.path = deploy-rs.lib."${system}".activate.nixos
                self.nixosConfigurations.server;
            };
            profile = {
              hostname = "server";
              sshUser = "${user}";
              sshOpts = [
                "-o"
                "UserKnownHostsFile=/dev/null"
                "-o"
                "StrictHostKeyChecking=no"
              ];
              profiles = {
                "hello-world".path =
                  let
                    activateProfile = pkgs.writeShellScriptBin "activate" ''
                      set -euo pipefail
                      mkdir -p /home/${user}/.nix-profile/bin
                      rm -f -- /home/${user}/.nix-profile/bin/hello /home/${user}/.nix-profile/bin/figlet
                      ln -s ${pkgs.hello}/bin/hello /home/${user}/.nix-profile/bin/hello
                      ln -s ${pkgs.figlet}/bin/figlet /home/${user}/.nix-profile/bin/figlet
                    '';
                  in
                  deploy-rs.lib.${system}.activate.custom activateProfile
                    "$PROFILE/bin/activate";
              };
            };
          };
        };
    };
}
