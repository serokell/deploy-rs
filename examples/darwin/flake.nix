{
  description = "Deploy simple 'darwinSystem' to a darwin machine";

  inputs.deploy-rs.url = "github:serokell/deploy-rs";
  inputs.darwin.url = "github:LnL7/nix-darwin";

  outputs =
    {
      self,
      deploy-rs,
      darwin,
    }:
    {
      darwinConfigurations.example = darwin.lib.darwinSystem {
        system = "x86_64-darwin";
        modules = [
          (
            {
              ...
            }:
            {
              services.nix-daemon.enable = true;
              nix = {
                settings = {
                  trusted-users = [ "rvem" ];
                };
                extraOptions = ''
                  experimental-features = flakes nix-command
                '';
              };
              # nix commands are added to PATH in the zsh config
              programs.zsh.enable = true;
            }
          )
        ];
      };
      deploy = {
        # remoteBuild = true; # Uncomment in case the system you're deploying from is not darwin
        nodes.example = {
          hostname = "localhost";
          profiles.system = {
            user = "root";
            path = deploy-rs.lib.x86_64-darwin.activate.darwin self.darwinConfigurations.example;
          };
        };
      };

      checks = builtins.mapAttrs (system: deployLib: deployLib.deployChecks self.deploy) deploy-rs.lib;
    };
}
