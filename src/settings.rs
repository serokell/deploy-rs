// SPDX-FileCopyrightText: 2020 Serokell <https://serokell.io/>
//
// SPDX-License-Identifier: MPL-2.0

use clap::Parser;
use envmnt::{self, ExpandOptions, ExpansionType};
use merge::Merge;
use serde::{Deserialize, Deserializer};
use std::collections::HashMap;

#[derive(Parser, Deserialize, Debug, Clone, Merge, Default)]
pub struct GenericSettings {
    /// Override the SSH user with the given value
    #[clap(long)]
    #[serde(rename(deserialize = "sshUser"))]
    pub ssh_user: Option<String>,
    /// Override the profile user with the given value
    #[clap(long = "profile-user")]
    pub user: Option<String>,
    /// Override the SSH options used
    #[clap(long, multiple_occurrences(true))]
    #[serde(
        skip_serializing_if = "Vec::is_empty",
        default,
        rename(deserialize = "sshOpts"),
        deserialize_with = "GenericSettings::de_ssh_opts"
    )]
    #[merge(strategy = merge::vec::append)]
    pub ssh_opts: Vec<String>,
    /// Override if the connecting to the target node should be considered fast
    #[clap(long)]
    #[serde(rename(deserialize = "fastConnection"), default)]
    #[merge(strategy = merge::bool::overwrite_false)]
    pub fast_connection: bool,
    /// Do not attempt rollback if activation fails
    #[clap(long)]
    #[serde(rename(deserialize = "noAutoRollback"), default)]
    #[merge(strategy = merge::bool::overwrite_true)]
    pub auto_rollback: bool,
    /// How long activation should wait for confirmation (if using magic-rollback)
    #[clap(long)]
    #[serde(rename(deserialize = "confirmTimeout"))]
    pub confirm_timeout: Option<u16>,
    /// Where to store temporary files (only used by magic-rollback)
    #[clap(long)]
    #[serde(rename(deserialize = "tempPath"))]
    pub temp_path: Option<String>,
    /// Do not do a magic rollback (see documentation)
    #[clap(long)]
    #[serde(rename(deserialize = "noMagicRollback"), default)]
    #[merge(strategy = merge::bool::overwrite_true)]
    pub magic_rollback: bool,
}

impl GenericSettings {
    fn de_ssh_opts<'de, D>(deserializer: D) -> Result<Vec<String>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let buf: Vec<String> = Vec::deserialize(deserializer)?;

        let mut options = ExpandOptions::new();
        options.expansion_type = Some(ExpansionType::UnixBrackets);

        Ok(buf
            .into_iter()
            .map(|opt| envmnt::expand(&opt, Some(options)))
            .collect())
    }
}

#[derive(Deserialize, Debug, Clone)]
pub struct NodeSettings {
    pub hostname: Option<String>,
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
