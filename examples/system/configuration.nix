# SPDX-FileCopyrightText: 2020 Serokell <https://serokell.io/>
#
# SPDX-License-Identifier: MPL-2.0

{
  imports = [ ./common.nix ];

  networking.hostName = "example-nixos-system";

  users.users.hello = {
    isNormalUser = true;
    password = "";
    uid = 1010;
  };
}
