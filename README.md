<!--
SPDX-FileCopyrightText: 2020 Serokell <https://serokell.io/>

SPDX-License-Identifier: MPL-2.0
-->

# deploy-rs

A Simple, multi-profile Nix-flake deploy tool.

**This is very early development software, you should expect to find issues, and things will change**

## Usage

Basic usage: `deploy [options] <flake>`.

The given flake can be just a source `my-flake`, or optionally specify the node to deploy `my-flake#my-node`, or specify a profile too `my-flake#my-node.my-profile`.

You can try out this tool easily with `nix run`:
- `nix run github:serokell/deploy-rs your-flake`

If you require a signing key to push closures to your server, specify the path to it in the `LOCAL_KEY` environment variable.

Check out `deploy --help` for CLI flags! Remember to check there before making one-time changes to things like hostnames.

## API

### Profile

This is the core of how `deploy-rs` was designed, any number of these can run on a node, as any user (see further down for specifying user information). If you want to mimick the behaviour of traditional tools like NixOps or Morph, try just defining one `profile` called `system`, as root, containing a nixosSystem, and you can even similarly use [home-manager](https://github.com/nix-community/home-manager) on any non-privileged user.

```nix
{
  # The command to bootstrap your profile, this is optional
  bootstrap = "mkdir xyz";

  # A derivation containing your required software, and a script to activate it in `${path}/activate`
  # For ease of use, `deploy-rs` provides a function to easy all this required activation script to any derivation
  path = deploy-rs.lib.x86_64-linux.setActivate pkgs.hello "./bin/hello";

  # An optional path to where your profile should be installed to, this is useful if you want to use a common profile name across multiple users, but would have conflicts in your node's profile list.
  profilePath = "/nix/var/nix/profiles/per-user/someuser/someprofile";

  # ...generic options... (see lower section)
}
```

### Node

This defines a single node/server, and the profiles you intend it to run.

```nix
{
  # The hostname of your server, don't worry, this can be overridden at runtime if needed
  hostname = "my.server.gov";

  # An optional list containing the order you want profiles to be deployed.
  profilesOrder = [ "something" "system" ];

  profiles = {
    system = {}; # Definition shown above
    something = {}; # Definition shown above
  };

  # ...generic options... (see lower section)
}
```

### Deploy

This is the top level attribute containing all of the options for this tool

```nix
{
  nodes = {
    my-node = {}; # Definition shown above
    another-node = {}; # Definition shown above
  };

  # ...generic options... (see lower section)
}
```

### Generic options

This is a set of options that can be put in any of the above definitions, with the priority being `profile > node > deploy`

```nix
{
  sshUser = "admin"; # This is the user that deploy-rs will use when connecting
  user = "root"; # This is the user that the profile will be deployed to (will use sudo if not the same as above)
  sshOpts = [ "-p" "2121" ]; # These are arguments that will be passed to SSH
  fastConnection = false; # Fast connection to the node. If this is true, copy the whole closure instead of letting the node substitute
  autoRollback = true; # If the previous profile should be re-activated if activation fails
}
```

A stronger definition of the schema is in the [interface directory](./interface), and full working examples Nix expressions/configurations are in the [examples folder](./examples).

## Idea

`deploy-rs` is a simple Rust program that will take a Nix flake and use it to deploy any of your defined profiles to your nodes. This is _strongly_ based off of [serokell/deploy](https://github.com/serokell/deploy), designed to replace it and expand upon it.

This type of design (as opposed to more traditional tools like NixOps or morph) allows for lesser-privileged deployments, and the ability to update different things independently of eachother.

## Things to work on

- ~~Ordered profiles~~
- ~~Automatic rollbacks~~
- UI (?)
- automatic kexec lustration of servers (maybe)
- Remote health checks
- Rollback on reconnection failure (technically, rollback if not reconnected to)

## About Serokell

deploy-rs is maintained and funded with ❤️ by [Serokell](https://serokell.io/).
The names and logo for Serokell are trademark of Serokell OÜ.

We love open source software! See [our other projects](https://serokell.io/community?utm_source=github) or [hire us](https://serokell.io/hire-us?utm_source=github) to design, develop and grow your idea!