<!--
SPDX-FileCopyrightText: 2023 Serokell <https://serokell.io/>

SPDX-License-Identifier: MPL-2.0
-->

# Example nix-darwin system deployment

## Prerequisites

1) Install `nix` and `nix-darwin` (the latter creates `/run` sets up `/etc/nix/nix.conf` symlink and so on)
   on the target machine.
2) Enable remote login on the mac to allow ssh access.
3) `deploy-rs` doesn't support password provisioning for `sudo`, so the `sshUser` should
   have passwordless `sudo` access.

## Deploying

Run `nix run github:serokell/deploy-rs -- --ssh-user <user>`.
