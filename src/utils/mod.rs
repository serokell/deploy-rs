// SPDX-FileCopyrightText: 2020 Serokell <https://serokell.io/>
// SPDX-FileCopyrightText: 2020 Andreas Fuchs <asf@boinkor.net>
//
// SPDX-License-Identifier: MPL-2.0

use rnix::{types::*, NodeOrToken, SyntaxKind::*, SyntaxNode};

use std::path::PathBuf;

use merge::Merge;

use thiserror::Error;

#[macro_export]
macro_rules! good_panic {
    ($($tts:tt)*) => {{
        error!($($tts)*);
        std::process::exit(1);
    }}
}

pub mod data;
pub mod deploy;
pub mod push;

#[derive(Debug)]
pub struct CmdOverrides {
    pub ssh_user: Option<String>,
    pub profile_user: Option<String>,
    pub ssh_opts: Option<String>,
    pub fast_connection: Option<bool>,
    pub auto_rollback: Option<bool>,
    pub hostname: Option<String>,
    pub magic_rollback: Option<bool>,
    pub temp_path: Option<String>,
    pub confirm_timeout: Option<u16>,
}

#[derive(PartialEq, Debug)]
pub struct DeployFlake<'a> {
    pub repo: &'a str,
    pub node: Option<String>,
    pub profile: Option<String>,
}

#[derive(Error, Debug)]
pub enum ParseFlakeError {
    #[error("The given path was too long, did you mean to put something in quotes?")]
    PathTooLong,
    #[error("Unrecognized node or token encountered")]
    Unrecognized,
}
pub fn parse_flake(flake: &str) -> Result<DeployFlake, ParseFlakeError> {
    let flake_fragment_start = flake.find('#');
    let (repo, maybe_fragment) = match flake_fragment_start {
        Some(s) => (&flake[..s], Some(&flake[s + 1..])),
        None => (flake, None),
    };

    let mut node: Option<String> = None;
    let mut profile: Option<String> = None;

    if let Some(fragment) = maybe_fragment {
        let ast = rnix::parse(fragment);

        let first_child = match ast.root().node().first_child() {
            Some(x) => x,
            None => {
                return Ok(DeployFlake {
                    repo,
                    node: None,
                    profile: None,
                })
            }
        };

        let mut node_over = false;

        for entry in first_child.children_with_tokens() {
            let x: Option<String> = match (entry.kind(), node_over) {
                (TOKEN_DOT, false) => {
                    node_over = true;
                    None
                }
                (TOKEN_DOT, true) => {
                    return Err(ParseFlakeError::PathTooLong);
                }
                (NODE_IDENT, _) => Some(entry.into_node().unwrap().text().to_string()),
                (TOKEN_IDENT, _) => Some(entry.into_token().unwrap().text().to_string()),
                (NODE_STRING, _) => {
                    let c = entry
                        .into_node()
                        .unwrap()
                        .children_with_tokens()
                        .nth(1)
                        .unwrap();

                    Some(c.into_token().unwrap().text().to_string())
                }
                _ => return Err(ParseFlakeError::Unrecognized),
            };

            if !node_over {
                node = x;
            } else {
                profile = x;
            }
        }
    }

    Ok(DeployFlake {
        repo,
        node,
        profile,
    })
}

#[test]
fn test_parse_flake() {
    assert_eq!(
        parse_flake("../deploy/examples/system").unwrap(),
        DeployFlake {
            repo: "../deploy/examples/system",
            node: None,
            profile: None,
        }
    );

    assert_eq!(
        parse_flake("../deploy/examples/system#").unwrap(),
        DeployFlake {
            repo: "../deploy/examples/system",
            node: None,
            profile: None,
        }
    );

    assert_eq!(
        parse_flake("../deploy/examples/system#computer.\"something.nix\"").unwrap(),
        DeployFlake {
            repo: "../deploy/examples/system",
            node: Some("computer".to_string()),
            profile: Some("something.nix".to_string()),
        }
    );

    assert_eq!(
        parse_flake("../deploy/examples/system#\"example.com\".system").unwrap(),
        DeployFlake {
            repo: "../deploy/examples/system",
            node: Some("example.com".to_string()),
            profile: Some("system".to_string()),
        }
    );

    assert_eq!(
        parse_flake("../deploy/examples/system#example").unwrap(),
        DeployFlake {
            repo: "../deploy/examples/system",
            node: Some("example".to_string()),
            profile: None
        }
    );

    assert_eq!(
        parse_flake("../deploy/examples/system#example.system").unwrap(),
        DeployFlake {
            repo: "../deploy/examples/system",
            node: Some("example".to_string()),
            profile: Some("system".to_string())
        }
    );

    assert_eq!(
        parse_flake("../deploy/examples/system").unwrap(),
        DeployFlake {
            repo: "../deploy/examples/system",
            node: None,
            profile: None,
        }
    );
}

#[derive(Debug, Clone)]
pub struct DeployData<'a> {
    pub node_name: &'a str,
    pub node: &'a data::Node,
    pub profile_name: &'a str,
    pub profile: &'a data::Profile,

    pub cmd_overrides: &'a CmdOverrides,

    pub merged_settings: data::GenericSettings,
}

#[derive(Debug)]
pub struct DeployDefs {
    pub ssh_user: String,
    pub profile_user: String,
    pub profile_path: String,
    pub sudo: Option<String>,
}

#[derive(Error, Debug)]
pub enum DeployDataDefsError {
    #[error("Neither `user` nor `sshUser` are set for profile {0} of node {1}")]
    NoProfileUser(String, String),
}

impl<'a> DeployData<'a> {
    pub fn defs(&'a self) -> Result<DeployDefs, DeployDataDefsError> {
        let ssh_user = match self.merged_settings.ssh_user {
            Some(ref u) => u.clone(),
            None => whoami::username(),
        };

        let profile_user = match self.merged_settings.user {
            Some(ref x) => x.clone(),
            None => match self.merged_settings.ssh_user {
                Some(ref x) => x.clone(),
                None => {
                    return Err(DeployDataDefsError::NoProfileUser(
                        self.profile_name.to_owned(),
                        self.node_name.to_owned(),
                    ))
                }
            },
        };

        let profile_path = match self.profile.profile_settings.profile_path {
            None => match &profile_user[..] {
                "root" => format!("/nix/var/nix/profiles/{}", self.profile_name),
                _ => format!(
                    "/nix/var/nix/profiles/per-user/{}/{}",
                    profile_user, self.profile_name
                ),
            },
            Some(ref x) => x.clone(),
        };

        let sudo: Option<String> = match self.merged_settings.user {
            Some(ref user) if user != &ssh_user => Some(format!("sudo -u {}", user)),
            _ => None,
        };

        Ok(DeployDefs {
            ssh_user,
            profile_user,
            profile_path,
            sudo,
        })
    }
}

pub fn make_deploy_data<'a, 's>(
    top_settings: &'s data::GenericSettings,
    node: &'a data::Node,
    node_name: &'a str,
    profile: &'a data::Profile,
    profile_name: &'a str,
    cmd_overrides: &'a CmdOverrides,
) -> DeployData<'a> {
    let mut merged_settings = profile.generic_settings.clone();
    merged_settings.merge(node.generic_settings.clone());
    merged_settings.merge(top_settings.clone());

    if cmd_overrides.ssh_user.is_some() {
        merged_settings.ssh_user = cmd_overrides.ssh_user.clone();
    }
    if cmd_overrides.profile_user.is_some() {
        merged_settings.user = cmd_overrides.profile_user.clone();
    }
    if let Some(ref ssh_opts) = cmd_overrides.ssh_opts {
        merged_settings.ssh_opts = ssh_opts.split(' ').map(|x| x.to_owned()).collect();
    }
    if let Some(fast_connection) = cmd_overrides.fast_connection {
        merged_settings.fast_connection = Some(fast_connection);
    }
    if let Some(auto_rollback) = cmd_overrides.auto_rollback {
        merged_settings.auto_rollback = Some(auto_rollback);
    }
    if let Some(magic_rollback) = cmd_overrides.magic_rollback {
        merged_settings.magic_rollback = Some(magic_rollback);
    }

    DeployData {
        profile,
        profile_name,
        node,
        node_name,

        cmd_overrides,

        merged_settings,
    }
}

#[derive(Error, Debug)]
pub enum DeployPathToActivatePathError {
    #[error("Deploy path did not have a parent directory")]
    PathTooShort,
    #[error("Deploy path was not valid utf8")]
    InvalidUtf8,
}
