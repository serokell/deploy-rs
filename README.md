<!--
SPDX-FileCopyrightText: 2020 Serokell <https://serokell.io/>

SPDX-License-Identifier: MPL-2.0
-->

# deploy-rs

A Simple multi-profile Nix-flake deploy tool.

**This is very early development software, you should expect to find issues**

## Usage


- `nix run github:serokell/deploy-rs your-flake#node.profile`
- `nix run github:serokell/deploy-rs your-flake#node`
- `nix run github:serokell/deploy-rs your-flake`

## API

Example Nix expressions/configurations are in the [examples folder](./examples).

## Idea

`deploy-rs` is a simple Rust program that will take a Nix flake and use it to deploy any of your defined profiles to your nodes. This is _strongly_ based off of [serokell/deploy](https://github.com/serokell/deploy), with the intention of eventually replacing it.

This type of design (as opposed to more traditional tools like NixOps or morph) allows for lesser-privileged deployments, and the ability to update different things independently of eachother.

## Things to work on

- ~~Ordered profiles~~
- Automatic rollbacks if one profile on node failed to deploy (partially implemented)
- UI (?)

## About Serokell

deploy-rs is maintained and funded with ❤️ by [Serokell](https://serokell.io/).
The names and logo for Serokell are trademark of Serokell OÜ.

We love open source software! See [our other projects](https://serokell.io/community?utm_source=github) or [hire us](https://serokell.io/hire-us?utm_source=github) to design, develop and grow your idea!