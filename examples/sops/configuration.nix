{ pkgs, ... }:
{
  networking.hostName = "sops";
  nix.settings = {
    # allow the `deploy` user to push unsigned NARs
    allowed-users = [ "deploy" ];
    trusted-users = [ "deploy" ];
  };

  # setup a user for the deployment
  users.users.deploy = {
    isNormalUser = true;
    password = "heloWorld";
    openssh.authorizedKeys.keys = [
      "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIFnXmG3pSC8+UfmrHH0L5UtT++KqTmLp+1B3oWIJ1IBB hello@localhost"
    ];
    extraGroups = [
      "wheel"
      "sudo"
    ]; # for sudo su
    uid = 1010;
  };

  # setup the rest of the system
  boot.loader = {
    systemd-boot.enable = true;
    efi.canTouchEfiVariables = true;
  };

  services.openssh.enable = true;

  nix.settings = {
    substituters = pkgs.lib.mkForce [ ];
    experimental-features = "nix-command flakes";
    trusted-public-keys = [ "cache.nixos.org-1:6NCHdD59X431o0gWypbMrAURkbJ16ZPMQFGspcDShjY=" ];
  };

  # settings for the vm
  virtualisation = {
    useBootLoader = true;
    writableStore = true;
    useEFIBoot = true;
  };
}
