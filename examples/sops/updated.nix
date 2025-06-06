{ lib, ... }:
let
  inherit (lib) mkForce;
in
{
  # update some config
  networking.hostName = mkForce "updated";
  users = {
    users.updated = {
      isNormalUser = true;
      password = "aReallyComplicatedPassword";
      uid = 1011;
      group = "updated";
    };
    groups.updated = { };
  };
}
