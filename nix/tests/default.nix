# SPDX-FileCopyrightText: 2024 Serokell <https://serokell.io/>
#
# SPDX-License-Identifier: MPL-2.0

{ pkgs , inputs , ... }:
let
  inherit (pkgs) lib;

  inherit (import "${pkgs.path}/nixos/tests/ssh-keys.nix" pkgs) snakeOilPrivateKey;

  # Include all build dependencies to be able to build profiles offline
  allDrvOutputs = pkg: pkgs.runCommand "allDrvOutputs" { refs = pkgs.writeClosure pkg.drvPath; } ''
    touch $out
    while read ref; do
      case $ref in
        *.drv)
          cat $ref >>$out
          ;;
      esac
    done <$refs
  '';

  mkTest = { name ? "", user ? "root", flakes ? true, isLocal ? true, deployArgs }: let
    nodes = {
      server = { nodes, ... }: {
        imports = [
         ./server.nix
         (import ./common.nix { inherit inputs pkgs flakes; })
        ];
        virtualisation.additionalPaths = lib.optionals (!isLocal) [
          pkgs.hello
          pkgs.figlet
          (allDrvOutputs nodes.server.system.build.toplevel)
          pkgs.deploy-rs.deploy-rs
        ];
      };
      client = { nodes, ... }: {
        imports = [ (import ./common.nix { inherit inputs pkgs flakes; }) ];
        environment.systemPackages = [ pkgs.deploy-rs.deploy-rs ];
        # nix evaluation takes a lot of memory, especially in non-flake usage
        virtualisation.memorySize = lib.mkForce 4096;
        virtualisation.additionalPaths = lib.optionals isLocal [
          pkgs.hello
          pkgs.figlet
          (allDrvOutputs nodes.server.system.build.toplevel)
        ];
      };
    };

    flakeInputs = ''
      deploy-rs.url = "${../..}";
      deploy-rs.inputs.utils.follows = "utils";
      deploy-rs.inputs.flake-compat.follows = "flake-compat";

      nixpkgs.url = "${inputs.nixpkgs}";
      utils.url = "${inputs.utils}";
      utils.inputs.systems.follows = "systems";
      systems.url = "${inputs.utils.inputs.systems}";
      flake-compat.url = "${inputs.flake-compat}";
      flake-compat.flake = false;

      enable-flakes.url = "${builtins.toFile "use-flakes" (if flakes then "true" else "false")}";
      enable-flakes.flake = false;
    '';

    flake = builtins.toFile "flake.nix"
      (lib.replaceStrings [ "##inputs##" ] [ flakeInputs ] (builtins.readFile ./deploy-flake.nix));

    flakeCompat = builtins.toFile "default.nix" ''
      (import
        (
          let
            lock = builtins.fromJSON (builtins.readFile ./flake.lock);
          in
          fetchTarball {
            url = "https://not-used-we-fetch-by-hash";
            sha256 = lock.nodes.flake-compat.locked.narHash;
          }
        )
        { src = ./.; }
      ).defaultNix
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
      client.succeed("cp ${flakeCompat} ./default.nix")
      client.succeed("cp ${./server.nix} ./server.nix")
      client.succeed("cp ${./common.nix} ./common.nix")
      client.succeed("cp ${serverNetworkJSON} ./network.json")
      client.succeed("nix --extra-experimental-features flakes flake lock")

      # Setup SSH key
      client.succeed("mkdir -m 700 /root/.ssh")
      client.succeed('cp --no-preserve=mode ${snakeOilPrivateKey} /root/.ssh/id_ed25519')
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
  # Deployment with overridden options
  options-overriding = mkTest {
    name = "options-overriding";
    deployArgs = lib.concatStrings [
      "-s .#server-override"
      " --hostname server --profile-user root --ssh-user root --sudo 'sudo -u'"
      " --ssh-opts='-o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null'"
      " --confirm-timeout 30 --activation-timeout 30"
      " -- --offline"
    ];
  };
  # User profile deployment
  profile = mkTest {
    name = "profile";
    user = "deploy";
    deployArgs = "-s .#profile -- --offline";
  };
  hyphen-ssh-opts-regression = mkTest {
    name = "profile";
    user = "deploy";
    deployArgs = "-s .#profile --ssh-opts '-p 22 -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null' -- --offline";
  };
  # Deployment using a non-flake nix
  non-flake-build = mkTest {
    name = "local-build";
    flakes = false;
    deployArgs = "-s .#server";
  };
  # Use node name with '.' symbol in it, regression for https://github.com/serokell/deploy-rs/issues/307
  node-name-with-dot = mkTest {
    name = "node-name-with-dot";
    deployArgs = "-s .#local.server -- --offline";
  };
}
