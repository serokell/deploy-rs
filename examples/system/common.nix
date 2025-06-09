# SPDX-FileCopyrightText: 2020 Serokell <https://serokell.io/>
#
# SPDX-License-Identifier: MPL-2.0

{
  users.users.admin = {
    isNormalUser = true;
    extraGroups = [
      "wheel"
      "sudo"
    ];
    password = "123";
  };

  services.openssh.enable = true;

  # Another option would be root on the server
  security.sudo.extraRules = [
    {
      groups = [ "wheel" ];
      commands = [
        {
          command = "ALL";
          options = [ "NOPASSWD" ];
        }
      ];
    }
  ];

  nix.settings = {
    # allow users in the weel group to upload unsigned nars
    trusted-users = [ "@wheel" ];
    trusted-public-keys = [ "cache.nixos.org-1:6NCHdD59X431o0gWypbMrAURkbJ16ZPMQFGspcDShjY=" ];
  };

  # these settings are needed in order for there to be a `/boot`
  boot.loader = {
    systemd-boot.enable = true;
    efi.canTouchEfiVariables = true;
  };

  # settings for the vm
  virtualisation = {
    useBootLoader = true;
    writableStore = true;
    useEFIBoot = true;
  };
}
