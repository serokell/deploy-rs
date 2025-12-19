// SPDX-FileCopyrightText: 2020 Serokell <https://serokell.io/>
//
// SPDX-License-Identifier: MPL-2.0

use merge::Merge;
use serde::Deserialize;
use std::collections::HashMap;
use std::path::PathBuf;

#[derive(Deserialize, Debug, Clone, Merge)]
pub struct GenericSettings {
    #[serde(rename(deserialize = "sshUser"))]
    pub ssh_user: Option<String>,
    pub user: Option<String>,
    #[serde(
        skip_serializing_if = "Vec::is_empty",
        default,
        rename(deserialize = "sshOpts")
    )]
    #[merge(strategy = merge::vec::append)]
    pub ssh_opts: Vec<String>,
    #[serde(rename(deserialize = "fastConnection"))]
    pub fast_connection: Option<bool>,
    #[serde(rename(deserialize = "autoRollback"))]
    pub auto_rollback: Option<bool>,
    #[serde(rename(deserialize = "confirmTimeout"))]
    pub confirm_timeout: Option<u16>,
    #[serde(rename(deserialize = "activationTimeout"))]
    pub activation_timeout: Option<u16>,
    #[serde(rename(deserialize = "tempPath"))]
    pub temp_path: Option<PathBuf>,
    #[serde(rename(deserialize = "magicRollback"))]
    pub magic_rollback: Option<bool>,
    #[serde(rename(deserialize = "sudo"))]
    pub sudo: Option<String>,
    #[serde(default,rename(deserialize = "remoteBuild"))]
    pub remote_build: Option<bool>,
    #[serde(rename(deserialize = "interactiveSudo"))]
    pub interactive_sudo: Option<bool>,
    // sops integration for secrets
    #[serde(rename(deserialize = "sudoFile"))]
    pub sudo_file: Option<PathBuf>,
    #[serde(rename(deserialize = "sudoSecret"))]
    pub sudo_secret: Option<String>,
}

#[derive(Deserialize, Debug, Clone)]
pub struct NodeSettings {
    pub hostname: String,
    pub profiles: HashMap<String, Profile>,
    #[serde(
        skip_serializing_if = "Vec::is_empty",
        default,
        rename(deserialize = "profilesOrder")
    )]
    pub profiles_order: Vec<String>,
}

#[derive(Deserialize, Debug, Clone)]
pub struct ProfileSettings {
    pub path: String,
    #[serde(rename(deserialize = "profilePath"))]
    pub profile_path: Option<String>,
}

#[derive(Deserialize, Debug, Clone)]
pub struct Profile {
    #[serde(flatten)]
    pub profile_settings: ProfileSettings,
    #[serde(flatten)]
    pub generic_settings: GenericSettings,
}

#[derive(Deserialize, Debug, Clone)]
pub struct Node {
    #[serde(flatten)]
    pub generic_settings: GenericSettings,
    #[serde(flatten)]
    pub node_settings: NodeSettings,
}

#[derive(Deserialize, Debug, Clone)]
pub struct Data {
    #[serde(flatten)]
    pub generic_settings: GenericSettings,
    pub nodes: HashMap<String, Node>,
}
