# SPDX-FileCopyrightText: 2024 Serokell <https://serokell.io/>
#
# SPDX-License-Identifier: MPL-2.0

{inputs, pkgs, flakes, ...}: {
  nix = {
    registry.nixpkgs.flake = inputs.nixpkgs;
    nixPath = [ "nixpkgs=${inputs.nixpkgs}" ];
    extraOptions = ''
      experimental-features = ${if flakes then "nix-command flakes" else "nix-command"}
    '';
    settings = {
      trusted-users = [ "root" "@wheel" ];
      substituters = pkgs.lib.mkForce [];
    };
  };

  # The "nixos-test-profile" profile disables the `switch-to-configuration` script by default
  system.switch.enable = true;

  virtualisation.graphics = false;
  virtualisation.memorySize = 1536;
  boot.loader.grub.enable = false;
  documentation.enable = false;
}
