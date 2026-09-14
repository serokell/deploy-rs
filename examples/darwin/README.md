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

## Troubleshooting

If you are running into a problem similar to this:

```
🚀 ℹ️ [deploy] [INFO] Building profile `system` for node `vm1`
🚀 ℹ️ [deploy] [INFO] Copying profile `system` to node `vm1`
(user@users-virtual-machine.local) Password:
🚀 ℹ️ [deploy] [INFO] Activating profile `system` for node `vm1`
🚀 ℹ️ [deploy] [INFO] Creating activation waiter
(user@users-virtual-machine.local) Password:
(user@users-virtual-machine.local) Password:
Received disconnect from fe80::1474:6f61:3c9b:a540%bridge100 port 22:2: Too many authentication failures
```

Try setting up **passwordless SSH login to the remote darwin guest** by adding your *host's public SSH key* to the *guest's `.ssh/authorized_keys`* file. Make sure to run `chmod -R go-rwx ~/.ssh` on the *guest*.
