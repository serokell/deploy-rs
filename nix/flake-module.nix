# SPDX-FileCopyrightText: 2024 Sefa Eyeoglu <contact@scrumplex.net>
#
# SPDX-License-Identifier: MPL-2.0

{lib, ...}: let
  inherit (lib) mkOption types;
  genericSettings = {
    options = {
      sshUser = mkOption {
        type = with types; nullOr str;
        default = null;
      };
      user = mkOption {
        type = with types; nullOr str;
        default = null;
      };
      sshOpts = mkOption {
        type = with types; listOf str;
        default = [];
      };
      fastConnection = mkOption {
        type = with types; nullOr bool;
        default = null;
      };
      autoRollback = mkOption {
        type = with types; nullOr bool;
        default = null;
      };
      confirmTimeout = mkOption {
        type = with types; nullOr int;
        default = null;
      };
      activationTimeout = mkOption {
        type = with types; nullOr int;
        default = null;
      };
      tempPath = mkOption {
        type = with types; nullOr str;
        default = null;
      };
      magicRollback = mkOption {
        type = with types; nullOr bool;
        default = null;
      };
      sudo = mkOption {
        type = with types; nullOr str;
        default = null;
      };
      remoteBuild = mkOption {
        type = with types; nullOr bool;
        default = null;
      };
      interactiveSudo = mkOption {
        type = with types; nullOr bool;
        default = null;
      };
    };
  };
  profileSettings = {
    options = {
      path = mkOption {
        type = types.package;
      };
      profilePath = mkOption {
        type = with types; nullOr str;
        default = null;
      };
    };
  };
  nodeSettings = {
    options = {
      hostname = mkOption {
        type = types.str;
      };
      profilesOrder = mkOption {
        type = with types; listOf str;
        default = [];
      };
      profiles = mkOption {
        type = types.attrsOf profileModule;
      };
    };
  };

  nodesSettings = {
    options.nodes = mkOption {
      type = types.attrsOf nodeModule;
    };
  };

  profileModule = types.submoduleWith {
    modules = [genericSettings profileSettings];
  };

  nodeModule = types.submoduleWith {
    modules = [genericSettings nodeSettings];
  };

  rootModule = types.submoduleWith {
    modules = [genericSettings nodesSettings];
  };
in {
  options.flake.deploy = mkOption {
    type = rootModule;
  };
}
