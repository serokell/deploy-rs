# deploy-rs
#### A candidate for [serokell/deploy](https://github.com/serokell/deploy)

**This is very early development software, you should expect to find issues**

## Usage examples

Example Nix expressions/configurations are in the [examples folder](./examples), here are various ways to deploy

- `nix run github:notgne2/deploy-rs your-flake#node.profile`
- `nix run github:notgne2/deploy-rs your-flake#node`
- `nix run github:notgne2/deploy-rs your-flake`

## Idea

`deploy-rs` is a simple Rust program that will take a Nix flake and use it to deploy any of your defined profiles to your nodes. This is _strongly_ based off of [serokell/deploy](https://github.com/serokell/deploy), with the intention of eventually replacing it.

This type of design (as opposed to more traditional tools like NixOps or morph) allows for lesser-privileged deployments, and the ability to update different things independently of eachother.

## Things to work on

- ~~Ordered profiles~~
- Automatic rollbacks if one profile on node failed to deploy (partially implemented)
- UI (?)