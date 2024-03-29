# SPDX-FileCopyrightText: 2024 Serokell <https://serokell.io/>
#
# SPDX-License-Identifier: MPL-2.0
{ pkgs, ... }:
{
  nix.settings.trusted-users = [ "deploy" ];
  users = let
    inherit (import "${pkgs.path}/nixos/tests/ssh-keys.nix" pkgs) snakeOilPublicKey;
  in {
    mutableUsers = false;
    users = {
      deploy = {
        password = "";
        isNormalUser = true;
        createHome = true;
        openssh.authorizedKeys.keys = [ snakeOilPublicKey ];
      };
      root.openssh.authorizedKeys.keys = [ snakeOilPublicKey ];
    };
  };
  services.openssh.enable = true;
  virtualisation.writableStore = true;
}
