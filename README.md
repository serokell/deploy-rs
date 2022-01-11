<!--
SPDX-FileCopyrightText: 2020 Serokell <https://serokell.io/>
SPDX-FileCopyrightText: 2021 Yannik Sander <contact@ysndr.de>

SPDX-License-Identifier: MPL-2.0
-->

![deploy-rs logo](./docs/logo.svg "deploy-rs")

---

A Simple, multi-profile Nix-flake deploy tool.

Questions? Need help? Join us on Matrix: [`#deploy-rs:matrix.org`](https://matrix.to/#/#deploy-rs:matrix.org)

## Usage

Basic usage: `deploy [options] <flake>`.

Using this method all profiles specified in the given `<flake>` will be deployed (taking into account the [`profilesOrder`](#node)).

 Optionally the flake can be constrained to deploy just a single node (`my-flake#my-node`) or a profile (`my-flake#my-node.my-profile`).

If your profile or node name has a . in it, simply wrap it in quotes, and the flake path in quotes (to avoid shell escaping), for example 'my-flake."myserver.com".system'.

Any "extra" arguments will be passed into the Nix calls, so for instance to deploy an impure profile, you may use `deploy . -- --impure` (note the explicit flake path is necessary for doing this).

You can try out this tool easily with `nix run`:
- `nix run github:serokell/deploy-rs your-flake`

If you want to deploy multiple flakes or a subset of profiles with one invocation, instead of calling `deploy <flake>` you can issue `deploy --targets <flake> [<flake> ...]` where `<flake>` is supposed to take the same format as discussed before.

Running in this mode, if any of the deploys fails, the deploy will be aborted and all successful deploys rolled back. `--rollback-succeeded false` can be used to override this behavior, otherwise the `auto-rollback` argument takes precedent.

If you require a signing key to push closures to your server, specify the path to it in the `LOCAL_KEY` environment variable.

Check out `deploy --help` for CLI flags! Remember to check there before making one-time changes to things like hostnames.

There is also an `activate` binary though this should be ignored, it is only used internally (on the deployed system) and for testing/hacking purposes.

## Ideas

`deploy-rs` is a simple Rust program that will take a Nix flake and use it to deploy any of your defined profiles to your nodes. This is _strongly_ based off of [serokell/deploy](https://github.com/serokell/deploy), designed to replace it and expand upon it.

### Multi-profile

This type of design (as opposed to more traditional tools like NixOps or morph) allows for lesser-privileged deployments, and the ability to update different things independently of eachother. You can deploy any type of profile to any user, not just a NixOS profile to `root`.

### Magic Rollback

There is a built-in feature to prevent you making changes that might render your machine unconnectable or unusuable, which works by connecting to the machine after profile activation to confirm the machine is still available, and instructing the target node to automatically roll back if it is not confirmed. If you do not disable `magicRollback` in your configuration (see later sections) or with the CLI flag, you will be unable to make changes to the system which will affect you connecting to it (changing SSH port, changing your IP, etc).

## API

### Overall usage

`deploy-rs` is designed to be used with Nix flakes (this currently requires an unstable version of Nix to work with). There is a Flake-less mode of operation which will automatically be used if your available Nix version does not support flakes, however you will likely want to use a flake anyway, just with `flake-compat` (see [this wiki page](https://nixos.wiki/wiki/Flakes) for usage).

`deploy-rs` also outputs a `lib` attribute, with tools used to make your definitions simpler and safer, including `deploy-rs.lib.${system}.activate` (see later section "Profile"), and `deploy-rs.lib.${system}.deployChecks` which will let `nix flake check` ensure your deployment is defined correctly.

There are full working deploy-rs Nix expressions in the [examples folder](./examples), and there is a JSON schema [here](./interface.json) which is used internally by the `deployChecks` mentioned above to validate your expressions.

A basic example of a flake that works with `deploy-rs` and deploys a simple NixOS configuration could look like this

```nix
{
  description = "Deployment for my server cluster";

  # For accessing `deploy-rs`'s utility Nix functions
  inputs.deploy-rs.url = "github:serokell/deploy-rs";

  outputs = { self, nixpkgs, deploy-rs }: {
    nixosConfigurations.some-random-system = nixpkgs.lib.nixosSystem {
      system = "x86_64-linux";
      modules = [ ./some-random-system/configuration.nix ];
    };

    deploy.nodes.some-random-system.profiles.system = {
        user = "root";
        path = deploy-rs.lib.x86_64-linux.activate.nixos self.nixosConfigurations.some-random-system;
    };

    # This is highly advised, and will prevent many possible mistakes
    checks = builtins.mapAttrs (system: deployLib: deployLib.deployChecks self.deploy) deploy-rs.lib;
  };
}
```

### Profile

This is the core of how `deploy-rs` was designed, any number of these can run on a node, as any user (see further down for specifying user information). If you want to mimic the behaviour of traditional tools like NixOps or Morph, try just defining one `profile` called `system`, as root, containing a nixosSystem, and you can even similarly use [home-manager](https://github.com/nix-community/home-manager) on any non-privileged user.

```nix
{
  # A derivation containing your required software, and a script to activate it in `${path}/deploy-rs-activate`
  # For ease of use, `deploy-rs` provides a function to easily add the required activation script to any derivation
  # Both the working directory and `$PROFILE` will point to `profilePath`
  path = deploy-rs.lib.x86_64-linux.activate.custom pkgs.hello "./bin/hello";

  # An optional path to where your profile should be installed to, this is useful if you want to use a common profile name across multiple users, but would have conflicts in your node's profile list.
  # This will default to `"/nix/var/nix/profiles/$PROFILE_NAME` if `user` is root (see: generic options), and `/nix/var/nix/profiles/per-user/$USER/$PROFILE_NAME` if it is not.
  profilePath = "/nix/var/nix/profiles/per-user/someuser/someprofile";

  # ...generic options... (see lower section)
}
```

### Node

This defines a single node/server, and the profiles you intend it to run.

```nix
{
  # The hostname of your server. Can be overridden at invocation time with a flag.
  hostname = "my.server.gov";

  # An optional list containing the order you want profiles to be deployed.
  # This will take effect whenever you run `deploy` without specifying a profile, causing it to deploy every profile automatically.
  # Any profiles not in this list will still be deployed (in an arbitrary order) after those which are listed
  profilesOrder = [ "something" "system" ];

  profiles = {
    # Definition format shown above
    system = {};
    something = {};
  };

  # ...generic options... (see lower section)
}
```

### Deploy

This is the top level attribute containing all of the options for this tool

```nix
{
  nodes = {
    # Definition format shown above
    my-node = {};
    another-node = {};
  };

  # ...generic options... (see lower section)
}
```

### Generic options

This is a set of options that can be put in any of the above definitions, with the priority being `profile > node > deploy`

```nix
{
  # This is the user that deploy-rs will use when connecting.
  # This will default to your own username if not specified anywhere
  sshUser = "admin";

  # This is the user that the profile will be deployed to (will use sudo if not the same as above).
  # If `sshUser` is specified, this will be the default (though it will _not_ default to your own username)
  user = "root";

  # This is an optional list of arguments that will be passed to SSH.
  sshOpts = [ "-p" "2121" ];

  # Fast connection to the node. If this is true, copy the whole closure instead of letting the node substitute.
  # This defaults to `false`
  fastConnection = false;

  # If the previous profile should be re-activated if activation fails.
  # This defaults to `true`
  autoRollback = true;

  # See the earlier section about Magic Rollback for more information.
  # This defaults to `true`
  magicRollback = true;

  # The path which deploy-rs will use for temporary files, this is currently only used by `magicRollback` to create an inotify watcher in for confirmations
  # If not specified, this will default to `/tmp`
  # (if `magicRollback` is in use, this _must_ be writable by `user`)
  tempPath = "/home/someuser/.deploy-rs";
}
```

## About Serokell

deploy-rs is maintained and funded with ❤️ by [Serokell](https://serokell.io/).
The names and logo for Serokell are trademark of Serokell OÜ.

We love open source software! See [our other projects](https://serokell.io/community?utm_source=github) or [hire us](https://serokell.io/hire-us?utm_source=github) to design, develop and grow your idea!
