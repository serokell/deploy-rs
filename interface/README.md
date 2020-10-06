A flake must have a `deploy` output with the following structure:

```
deploy
├── <generic args>
└── nodes
    ├── <NODE>
    │   ├── <generic args>
    │   ├── hostname
    │   └── profiles
    │       ├── <PROFILE>
    │       │   ├── <generic args>
    │       │   ├── bootstrap
    │       │   └── path
    │       └── <PROFILE>...
    └── <NODE>...

```

Where `<generic args>` are all optional and can be one or multiple of:

- `sshUser` -- user to connect as
- `user` -- user to install and activate profiles with
- `sshOpts` -- options passed to `nix copy` and `ssh`
- `fastConnection` -- whether the connection from this host to the target one is fast (if it is, don't substitute on target and copy the entire closure) [default: `false`]
- `autoRollback` -- whether to roll back when the deployment fails [default: `false`]

A formal definition for the structure can be found in [the JSON schema](./deploy.json)

For every profile of every node, arguments are merged with `<PROFILE>` taking precedence over `<NODE>` and `<NODE>` taking precedence over top-level.

Values can be overridden for all the profiles deployed by setting environment variables with the same names as the profile, for example `sshUser=foobar nix run github:serokell/deploy .` will connect to all nodes as `foobar@<NODE>.hostname`.
