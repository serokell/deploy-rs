# SPDX-FileCopyrightText: 2024 Serokell <https://serokell.io/>
#
# SPDX-License-Identifier: MPL-2.0

{
  inputs = {
    # real inputs are substituted in ./default.nix
##inputs##
  };

  outputs = { self, nixpkgs, deploy-rs, ... }@inputs: let
    system = "x86_64-linux";
    pkgs = inputs.nixpkgs.legacyPackages.${system};
    user = "deploy";
  in {
    nixosConfigurations.server = nixpkgs.lib.nixosSystem {
      inherit system pkgs;
      specialArgs = { inherit inputs; flakes = import inputs.enable-flakes; };
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
          "-o" "StrictHostKeyChecking=no"
          "-o" "StrictHostKeyChecking=no"
        ];
        profiles.system.path = deploy-rs.lib."${system}".activate.nixos self.nixosConfigurations.server;
      };
      server-override = {
        hostname = "override";
        sshUser = "override";
        user = "override";
        sudo = "override";
        sshOpts = [ ];
        confirmTimeout = 0;
        activationTimeout = 0;
        profiles.system.path = deploy-rs.lib."${system}".activate.nixos self.nixosConfigurations.server;
      };
      profile = {
        hostname = "server";
        sshUser = "${user}";
        sshOpts = [
          "-o" "UserKnownHostsFile=/dev/null"
          "-o" "StrictHostKeyChecking=no"
        ];
        profiles = {
          "hello-world".path = let
            activateProfile = pkgs.writeShellScriptBin "activate" ''
              set -euo pipefail
              mkdir -p /home/${user}/.nix-profile/bin
              rm -f -- /home/${user}/.nix-profile/bin/hello /home/${user}/.nix-profile/bin/figlet
              ln -s ${pkgs.hello}/bin/hello /home/${user}/.nix-profile/bin/hello
              ln -s ${pkgs.figlet}/bin/figlet /home/${user}/.nix-profile/bin/figlet
            '';
          in deploy-rs.lib.${system}.activate.custom activateProfile "$PROFILE/bin/activate";
        };
      };
      failing-server = {
        hostname = "server";
        sshUser = "root";
        sshOpts = [
          "-o" "UserKnownHostsFile=/dev/null"
          "-o" "StrictHostKeyChecking=no"
        ];
        profiles.system.path = let
          failingActivate = pkgs.writeShellScriptBin "activate" ''
            echo "intentional activation failure for cancellation test" >&2
            exit 1
          '';
        in deploy-rs.lib.${system}.activate.custom failingActivate "$PROFILE/bin/activate";
      };
      # Only exists for the prefix-demarcation e2e test (#261): gives it
      # predictable, distinct markers on each stream to look for, without
      # affecting what "-s .#profile" deploys for the other profile tests.
      prefix-demarcation = {
        hostname = "server";
        sshUser = "${user}";
        sshOpts = [
          "-o" "UserKnownHostsFile=/dev/null"
          "-o" "StrictHostKeyChecking=no"
        ];
        profiles."echo-markers".path = let
          activateProfile = pkgs.writeShellScriptBin "activate" ''
            echo "PREFIX_TEST_STDOUT_MARKER"
            echo "PREFIX_TEST_STDERR_MARKER" >&2
          '';
        in deploy-rs.lib.${system}.activate.custom activateProfile "$PROFILE/bin/activate";
      };
    };
  };
}
