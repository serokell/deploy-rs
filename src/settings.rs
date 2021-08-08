// SPDX-FileCopyrightText: 2020 Serokell <https://serokell.io/>
//
// SPDX-License-Identifier: MPL-2.0

use clap::Clap;
use merge::Merge;
use serde::Deserialize;
use std::collections::HashMap;

#[derive(Clap, Deserialize, Debug, Clone, Merge)]
pub struct GenericSettings {
    /// Override the SSH user with the given value
    #[clap(long)]
    #[serde(rename(deserialize = "sshUser"))]
    pub ssh_user: Option<String>,
    /// Override the profile user with the given value
    #[clap(long = "profile-user")]
    pub user: Option<String>,
    /// Override the SSH options used
    #[clap(long, multiple_occurrences(true), multiple_values(true))]
    #[serde(
        skip_serializing_if = "Vec::is_empty",
        default,
        rename(deserialize = "sshOpts")
    )]
    #[merge(strategy = merge::vec::append)]
    pub ssh_opts: Vec<String>,
    /// Override if the connecting to the target node should be considered fast
    #[clap(long)]
    #[serde(rename(deserialize = "fastConnection"))]
    pub fast_connection: Option<bool>,
    /// Override if a rollback should be attempted if activation fails
    #[clap(long)]
    #[serde(rename(deserialize = "autoRollback"))]
    pub auto_rollback: Option<bool>,
    /// How long activation should wait for confirmation (if using magic-rollback)
    #[clap(long)]
    #[serde(rename(deserialize = "confirmTimeout"))]
    pub confirm_timeout: Option<u16>,
    /// Where to store temporary files (only used by magic-rollback)
    #[clap(long)]
    #[serde(rename(deserialize = "tempPath"))]
    pub temp_path: Option<String>,
    #[clap(long)]
    #[serde(rename(deserialize = "magicRollback"))]
    pub magic_rollback: Option<bool>,
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
pub struct Root {
    #[serde(flatten)]
    pub generic_settings: GenericSettings,
    pub nodes: HashMap<String, Node>,
}
