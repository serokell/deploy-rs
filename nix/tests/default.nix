# SPDX-FileCopyrightText: 2024 Serokell <https://serokell.io/>
#
# SPDX-License-Identifier: MPL-2.0

{ pkgs , inputs , ... }:
let
  inherit (pkgs) system lib;

  privateKey = pkgs.writeText "privateKey" ''
    -----BEGIN OPENSSH PRIVATE KEY-----
    b3BlbnNzaC1rZXktdjEAAAAABG5vbmUAAAAEbm9uZQAAAAAAAAABAAAAMwAAAAtzc2gtZW
    QyNTUxOQAAACAhy9mfWtvOBI9XrpqatB7PkAF1snmcYJ8JICyx0FtuJgAAAIj35ajL9+Wo
    ywAAAAtzc2gtZWQyNTUxOQAAACAhy9mfWtvOBI9XrpqatB7PkAF1snmcYJ8JICyx0FtuJg
    AAAEB7FFmHl+KvJokiF4g2iq6a/6pzhepwQnLVZZdRAGRl0SHL2Z9a284Ej1eumpq0Hs+Q
    AXWyeZxgnwkgLLHQW24mAAAAAAECAwQF
    -----END OPENSSH PRIVATE KEY-----
  '';

  # Include all build dependencies to be able to build profiles offline
  allDrvOutputs = pkg: pkgs.runCommand "allDrvOutputs" { refs = pkgs.writeReferencesToFile pkg.drvPath; } ''
    touch $out
    while read ref; do
      case $ref in
        *.drv)
          cat $ref >>$out
          ;;
      esac
    done <$refs
  '';

  mkTest = { name ? "", user ? "root", isLocal ? true, deployArgs }: let
    nodes = {
      server = { nodes, ... }: {
        imports = [
         ./server.nix
         (import ./common.nix { inherit inputs pkgs; })
        ];
        virtualisation.additionalPaths = lib.optionals (!isLocal) [
          pkgs.hello
          pkgs.figlet
          (allDrvOutputs nodes.server.system.build.toplevel)
          pkgs.deploy-rs.deploy-rs
        ];
      };
      client = { nodes, ... }: {
        imports = [ (import ./common.nix { inherit inputs pkgs; }) ];
        environment.systemPackages = [ pkgs.deploy-rs.deploy-rs ];
        virtualisation.additionalPaths = lib.optionals isLocal [
          pkgs.hello
          pkgs.figlet
          (allDrvOutputs nodes.server.system.build.toplevel)
        ];
      };
    };

    flake = builtins.toFile "flake.nix" ''
      {
        inputs = {
          deploy-rs.url = "${../..}";
          deploy-rs.inputs.utils.follows = "utils";
          deploy-rs.inputs.flake-compat.follows = "flake-compat";

          nixpkgs.url = "${inputs.nixpkgs}";
          utils.url = "${inputs.utils}";
          utils.inputs.systems.follows = "systems";
          systems.url = "${inputs.utils.inputs.systems}";
          flake-compat.url = "${inputs.flake-compat}";
          flake-compat.flake = false;
        };

        outputs = { self, nixpkgs, deploy-rs, ... }@inputs: let
          system = "${system}";
          pkgs = inputs.nixpkgs.legacyPackages.${system};
        in {
          nixosConfigurations.server = nixpkgs.lib.nixosSystem {
            inherit system pkgs;
            modules = [
              ${builtins.readFile ./server.nix}
              ((${builtins.readFile ./common.nix}) { inherit inputs pkgs; })
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
              profiles.system.path = deploy-rs.lib."${system}".activate.nixos self.nixosConfigurations.server;
              sshOpts = [
                "-o" "UserKnownHostsFile=/dev/null"
                "-o" "StrictHostKeyChecking=no"
              ];
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
                  activateProfile = pkgs.writeShellScriptBin "activate" '''
                    set -euo pipefail
                    mkdir -p /home/${user}/.nix-profile/bin
                    rm -f -- /home/${user}/.nix-profile/bin/hello /home/${user}/.nix-profile/bin/figlet
                    ln -s ''${pkgs.hello}/bin/hello /home/${user}/.nix-profile/bin/hello
                    ln -s ''${pkgs.figlet}/bin/figlet /home/${user}/.nix-profile/bin/figlet
                  ''';
                in deploy-rs.lib.${system}.activate.custom activateProfile "$PROFILE/bin/activate";
              };
            };
          };
        };
      }
    '';
  in pkgs.nixosTest {
    inherit nodes name;

    testScript = { nodes }: let
      serverNetworkJSON = pkgs.writeText "server-network.json"
        (builtins.toJSON nodes.server.system.build.networkConfig);
    in ''
      start_all()

      # Prepare
      client.succeed("mkdir tmp && cd tmp")
      client.succeed("cp ${flake} ./flake.nix")
      client.succeed("cp ${serverNetworkJSON} ./network.json")
      client.succeed("nix flake lock")


      # Setup SSH key
      client.succeed("mkdir -m 700 /root/.ssh")
      client.succeed('cp --no-preserve=mode ${privateKey} /root/.ssh/id_ed25519')
      client.succeed("chmod 600 /root/.ssh/id_ed25519")

      # Test SSH connection
      server.wait_for_open_port(22)
      client.wait_for_unit("network.target")
      client.succeed(
        "ssh -o UserKnownHostsFile=/dev/null -o StrictHostKeyChecking=no server 'echo hello world' >&2",
        timeout=30
      )

      # Make sure the hello and figlet packages are missing
      server.fail("su ${user} -l -c 'hello | figlet'")

      # Deploy to the server
      client.succeed("deploy ${deployArgs}")

      # Make sure packages are present after deployment
      server.succeed("su ${user} -l -c 'hello | figlet' >&2")
    '';
  };
in {
  # Deployment with client-side build
  local-build = mkTest {
    name = "local-build";
    deployArgs = "-s .#server -- --offline";
  };
  # Deployment with server-side build
  remote-build = mkTest {
    name = "remote-build";
    isLocal = false;
    deployArgs = "-s .#server --remote-build -- --offline";
  };
  # User profile deployment
  profile = mkTest {
    name = "profile";
    user = "deploy";
    deployArgs = "-s .#profile -- --offline";
  };
}
