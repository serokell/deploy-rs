# SPDX-FileCopyrightText: 2020 Serokell <https://serokell.io/>
#
# SPDX-License-Identifier: MPL-2.0

nixpkgs:
let
  pkgs = nixpkgs.legacyPackages.x86_64-linux;
  generateSystemd = type: name: config:
    (nixpkgs.lib.nixosSystem {
      modules = [{ systemd."${type}s".${name} = config; }];
      system = "x86_64-linux";
    }).config.systemd.units."${name}.${type}".text;

  mkService = generateSystemd "service";

  service = pkgs.writeTextFile {
    name = "hello.service";
    text = mkService "hello" {
      unitConfig.WantedBy = [ "multi-user.target" ];
      path = [ pkgs.hello ];
      script = "hello";
    };
  };
in pkgs.writeShellScriptBin "activate" ''
  mkdir -p $HOME/.config/systemd/user
  rm $HOME/.config/systemd/user/hello.service
  ln -s ${service} $HOME/.config/systemd/user/hello.service
  systemctl --user daemon-reload
  systemctl --user restart hello
''
