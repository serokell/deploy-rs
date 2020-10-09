A flake must have a `deploy` output with the following structure:

```
deploy
├── <generic args>
└── nodes
    ├── <NODE>
    │   ├── <generic args>
    │   ├── hostname
    │   ├── profilesOrder
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

Certain read values can be overridden by supplying flags to the deploy binary, for example `deploy --auto-rollback true .` will enable automatic rollback for all nodes being deployed to, regardless of settings.