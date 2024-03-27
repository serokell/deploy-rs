# SPDX-FileCopyrightText: 2024 Serokell <https://serokell.io/>
#
# SPDX-License-Identifier: MPL-2.0

{
  nix.settings.trusted-users = [ "deploy" ];
  users = let
    pubkey = "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAICHL2Z9a284Ej1eumpq0Hs+QAXWyeZxgnwkgLLHQW24m";
  in {
    mutableUsers = false;
    users = {
      deploy = {
        password = "";
        isNormalUser = true;
        createHome = true;
        openssh.authorizedKeys.keys = [ pubkey ];
      };
      root.openssh.authorizedKeys.keys = [ pubkey ];
    };
  };
  services.openssh.enable = true;
  virtualisation.writableStore = true;
}
