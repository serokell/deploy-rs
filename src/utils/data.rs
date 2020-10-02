// SPDX-FileCopyrightText: 2020 Serokell <https://serokell.io/>
//
// SPDX-License-Identifier: MPL-2.0

use merge::Merge;

use std::collections::HashMap;

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
    #[serde(rename(deserialize = "fastConnection"), default)]
    #[merge(strategy = merge::bool::overwrite_false)]
    pub fast_connection: bool,
    #[serde(rename(deserialize = "autoRollback"), default)]
    #[merge(strategy = merge::bool::overwrite_false)]
    pub auto_rollback: bool,
}

#[derive(Deserialize, Debug, Clone)]
pub struct NodeSettings {
    pub hostname: String,
}

#[derive(Deserialize, Debug, Clone)]
pub struct ProfileSettings {
    pub path: String,
    pub bootstrap: Option<String>,
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

    pub profiles: HashMap<String, Profile>,
    #[serde(
        skip_serializing_if = "Vec::is_empty",
        default,
        rename(deserialize = "profilesOrder")
    )]
    pub profiles_order: Vec<String>,
}

#[derive(Deserialize, Debug, Clone)]
pub struct Data {
    #[serde(flatten)]
    pub generic_settings: GenericSettings,
    pub nodes: HashMap<String, Node>,
}
