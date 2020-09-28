{
  imports = [ ./common.nix ];

  networking.hostName = "example-nixos-syyyystem";

  users.users.hello = {
    isNormalUser = true;
    password = "";
    uid = 1010;
  };
}
