{
  imports = [ ./common.nix ];

  # Use that when deploy scripts asks you for a hostname
  networking.hostName = "bare-system";
}
