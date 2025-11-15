# Example NixOS system deployment where password is passed via sops

This is an example of how to use the sops integration for deploy-rs.

To decrypt the password manually use `SOPS_AGE_KEY_FILE=$(pwd)/age_private.txt sops -d passwords.yaml`.
Note that sops will try to search for the private key for age in `$XDG_CONFIG_HOME/sops/age/keys.txt` by default,
but this can be overridden with `SOPS_AGE_KEY_FILE`. For more information please see the [sops documentation](https://getsops.io/docs/#encrypting-using-age).

1. Run bare system from `.#nixosConfigurations.sops`

- `nix build .#nixosConfigurations.sops.config.system.build.vm`
- `QEMU_NET_OPTS=hostfwd=tcp::2221-:22 ./result/bin/run-sops-vm`
- you can manually ssh into the machine via `ssh deploy@localhost -p 2221 -i ./hello_ed25519`

2. Develop the devshell via `nix develop .` to get sops, age and `deploy` added to $PATH
3. Run via `deploy .` to deploy the "new" Configuration updated
4. ???
5. PROFIT!!!
