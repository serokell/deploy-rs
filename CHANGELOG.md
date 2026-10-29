<!--
SPDX-FileCopyrightText: 2026 Serokell <https://serokell.io/>

SPDX-License-Identifier: MPL-2.0
-->

# Changelog

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

## [Unreleased]

### Added

- `sudoCopy` (and `--sudo-copy`): copy the closure as root on the target,
  through the `sudo` command, so an `sshUser` that is not a trusted user of
  the target's Nix daemon can deploy unsigned paths. Sudo must not ask for a
  password; `sudoCopy` cannot be combined with `interactiveSudo`.

## [1.0.0] - 2026-10-19

### Added

- First versioned release of deploy-rs, and the first release on
  [crates.io](https://crates.io/crates/deploy-rs). (#407)

### Changed

- `deploy --version` and `activate --version` print the full version from
  `Cargo.toml`, `1.0.0`, instead of `1.0`. (#408)

[Unreleased]: https://github.com/serokell/deploy-rs/compare/v1.0.0...HEAD
[1.0.0]: https://github.com/serokell/deploy-rs/releases/tag/v1.0.0
