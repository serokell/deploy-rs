# SPDX-FileCopyrightText: 2026 Serokell <https://serokell.io/>
#
# SPDX-License-Identifier: MPL-2.0

# Drops paths for profiles that evaluate remotely. For other profiles, a `path` that
# evaluates to a derivation also exposes that derivation's `drvPath`. This
# lets deploy-rs build content-addressed and floating-output derivations,
# whose `outPath` is only a placeholder at eval time, without users having
# to set `drvPath` manually in their flake.
#
# The deploy-rs binary loads this file via `include_str!` from `src/cli.rs`
# and applies it. The expression is self-contained so it can be parsed in
# isolation for linting, and exercised by `nix/tests/transform-deploy.nix`.
remoteEvalOverride: deploy:
let
  patchProfile = n: p:
    let
      remoteEval = builtins.head (builtins.filter (x: x != null) [
        remoteEvalOverride
        (p.remoteEval or null)
        (n.remoteEval or null)
        (deploy.remoteEval or null)
        false
      ]);
    in
    if remoteEval then builtins.removeAttrs p [ "path" "drvPath" ]
    else if (p ? path) && (builtins.isAttrs p.path) && (p.path ? drvPath)
    then p // { path = p.path.outPath; drvPath = p.path.drvPath; }
    else p;
  patchNode = n: n // {
    profiles = builtins.mapAttrs (_: patchProfile n) (n.profiles or { });
  };
in
if deploy ? nodes
then deploy // { nodes = builtins.mapAttrs (_: patchNode) deploy.nodes; }
else deploy
