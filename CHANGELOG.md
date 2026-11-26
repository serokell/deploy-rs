<!--
SPDX-FileCopyrightText: 2026 Serokell <https://serokell.io/>

SPDX-License-Identifier: MPL-2.0
-->

# Changelog

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

## [Unreleased]

### Added

- Added a `--no-emoji` flag for `deploy` and `activate`. It removes the emoji
  from log lines and progress bars. Forwarded remote output gets the `[remote]`
  prefix in place of `📠`. `deploy` passes the flag on to `activate` on the
  target. That `activate` comes from the deploy-rs version the deployed flake
  pins. If that version is older than this release, activation fails with an
  "unexpected argument" error. (#360)
- Added `OutputOptions`, and the library functions `init_logger_with_options`,
  `deploy::deploy_profile_with_options`, and `deploy::revoke_with_options` that
  take it. `init_logger`, `deploy_profile`, and `revoke` keep their 1.0.0
  signatures and print emoji as before. (#360)

## [1.0.0] - 2026-10-19

### Added

- First versioned release of deploy-rs, and the first release on
  [crates.io](https://crates.io/crates/deploy-rs). (#407)

### Changed

- `deploy --version` and `activate --version` print the full version from
  `Cargo.toml`, `1.0.0`, instead of `1.0`. (#408)

[Unreleased]: https://github.com/serokell/deploy-rs/compare/v1.0.0...HEAD
[1.0.0]: https://github.com/serokell/deploy-rs/releases/tag/v1.0.0
