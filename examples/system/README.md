<!--
SPDX-FileCopyrightText: 2020 Serokell <https://serokell.io/>

SPDX-License-Identifier: MPL-2.0
-->

# Example nixos system deployment

This is an example of how to deploy a full nixos system with a separate user unit to a bare machine.

1. Run bare system from `.#nixosConfigurations.bare`
  - `nix build .#nixosConfigurations.bare.config.system.build.vm`
  - `QEMU_NET_OPTS=hostfwd=tcp::2221-:22 ./result/bin/run-bare-system-vm`
2. `nix run github:serokell/yeet`
3. ???
4. PROFIT!!!
