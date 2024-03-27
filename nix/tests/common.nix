# SPDX-FileCopyrightText: 2024 Serokell <https://serokell.io/>
#
# SPDX-License-Identifier: MPL-2.0

{inputs, pkgs, ...}: {
  nix = {
    registry.nixpkgs.flake = inputs.nixpkgs;
    extraOptions = ''
      experimental-features = nix-command flakes
    '';
    settings = {
      trusted-users = [ "root" "@wheel" ];
      substituters = pkgs.lib.mkForce [];
    };
  };

  virtualisation.graphics = false;
  boot.loader.grub.enable = false;
  documentation.enable = false;
}
