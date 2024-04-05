# SPDX-FileCopyrightText: 2024 Sefa Eyeoglu <contact@scrumplex.net>
#
# SPDX-License-Identifier: MPL-2.0

{
  description = "Deploy GNU hello to localhost";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-parts.url = "github:hercules-ci/flake-parts";
    deploy-rs.url = "github:serokell/deploy-rs";
  };

  outputs = inputs@{ flake-parts, ... }:
    flake-parts.lib.mkFlake { inherit inputs; } {
      imports = [inputs.deploy-rs.flakeModule];

      flake.deploy.nodes.example = {
        hostname = "localhost";
        profiles.hello = {
          user = "balsoft";
          path = inputs.deploy-rs.lib.x86_64-linux.setActivate inputs.nixpkgs.legacyPackages.x86_64-linux.hello "./bin/hello";
        };
      };
      systems = [
        # systems for which you want to build the `perSystem` attributes
        "x86_64-linux"
      ];
    };
}
