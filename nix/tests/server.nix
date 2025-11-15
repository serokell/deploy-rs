# SPDX-FileCopyrightText: 2024 Serokell <https://serokell.io/>
#
# SPDX-License-Identifier: MPL-2.0
{ pkgs, ... }:
{
  nix.settings.trusted-users = [ "deploy" ];
  users =
    let
      inherit (import "${pkgs.path}/nixos/tests/ssh-keys.nix" pkgs) snakeOilPublicKey;
    in
    {
      mutableUsers = false;
      users = {
        deploy = {
          password = "";
          isNormalUser = true;
          createHome = true;
          group = "deploy";
          extraGroups = [ "wheel" ]; # need wheel for `sudo su`
          openssh.authorizedKeys.keys = [ snakeOilPublicKey ];
        };
        sops = {
          password = "rootIsAGoodRootPassword";
          isNormalUser = true;
          createHome = true;
          group = "sops";
          extraGroups = [ "wheel" ]; # need wheel for `sudo su`
          openssh.authorizedKeys.keys = [ snakeOilPublicKey ];
        };
        root.openssh.authorizedKeys.keys = [ snakeOilPublicKey ];
      };
      groups = {
        deploy = { };
        sops = { };
      };
    };

  # deploy does not need a password for sudo
  security.sudo.extraRules = [
    {
      groups = [ "deploy" ];
      commands = [
        {
          command = "ALL";
          options = [ "NOPASSWD" ];
        }
      ];
    }
  ];
  services.openssh.enable = true;
  virtualisation.writableStore = true;
}
