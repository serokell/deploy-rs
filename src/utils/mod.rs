// SPDX-FileCopyrightText: 2020 Serokell <https://serokell.io/>
//
// SPDX-License-Identifier: MPL-2.0

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
    pub node: Option<&'a str>,
    pub profile: Option<&'a str>,
}

pub fn parse_flake(flake: &str) -> DeployFlake {
    let flake_fragment_start = flake.find('#');
    let (repo, maybe_fragment) = match flake_fragment_start {
        Some(s) => (&flake[..s], Some(&flake[s + 1..])),
        None => (flake, None),
    };

    let (node, profile) = match maybe_fragment {
        Some(fragment) => {
            let fragment_profile_start = fragment.rfind('.');

            match fragment_profile_start {
                Some(s) => (
                    Some(&fragment[..s]),
                    // Ignore the trailing `.`
                    (if (s + 1) == fragment.len() {
                        None
                    } else {
                        Some(&fragment[s + 1..])
                    }),
                ),
                None => (Some(fragment), None),
            }
        }
        None => (None, None),
    };

    DeployFlake {
        repo,
        node,
        profile,
    }
}

#[test]
fn test_parse_flake() {
    assert_eq!(
        parse_flake("../deploy/examples/system#example"),
        DeployFlake {
            repo: "../deploy/examples/system",
            node: Some("example"),
            profile: None
        }
    );

    assert_eq!(
        parse_flake("../deploy/examples/system#example.system"),
        DeployFlake {
            repo: "../deploy/examples/system",
            node: Some("example"),
            profile: Some("system")
        }
    );

    assert_eq!(
        parse_flake("../deploy/examples/system"),
        DeployFlake {
            repo: "../deploy/examples/system",
            node: None,
            profile: None,
        }
    );

    // Trailing `.` should be ignored
    assert_eq!(
        parse_flake("../deploy/examples/system#example."),
        DeployFlake {
            repo: "../deploy/examples/system",
            node: Some("example"),
            profile: None
        }
    );

    // The last `.` should be used for splitting
    assert_eq!(
        parse_flake("../deploy/examples/system#example.com.system"),
        DeployFlake {
            repo: "../deploy/examples/system",
            node: Some("example.com"),
            profile: Some("system")
        }
    );

    // The last `.` should be used for splitting, _and_ trailing `.` should be ignored
    assert_eq!(
        parse_flake("../deploy/examples/system#example.com."),
        DeployFlake {
            repo: "../deploy/examples/system",
            node: Some("example.com"),
            profile: None
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
    pub current_exe: PathBuf,
    pub sudo: Option<String>,
}

#[derive(Error, Debug)]
pub enum DeployDataDefsError {
    #[error("Neither `user` nor `sshUser` are set for profile {0} of node {1}")]
    NoProfileUser(String, String),
    #[error("Error reading current executable path: {0}")]
    ExecutablePathNotFound(std::io::Error),
    #[error("Executable was not in the Nix store")]
    NotNixStored,
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

        let current_exe =
            std::env::current_exe().map_err(DeployDataDefsError::ExecutablePathNotFound)?;

        if !current_exe.starts_with("/nix/store/") {
            return Err(DeployDataDefsError::NotNixStored);
        }

        Ok(DeployDefs {
            ssh_user,
            profile_user,
            profile_path,
            current_exe,
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
    let mut merged_settings = top_settings.clone();
    merged_settings.merge(node.generic_settings.clone());
    merged_settings.merge(profile.generic_settings.clone());

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

pub fn deploy_path_to_activate_path_str(
    deploy_path: &std::path::Path,
) -> Result<String, DeployPathToActivatePathError> {
    Ok(format!(
        "{}/activate",
        deploy_path
            .parent()
            .ok_or(DeployPathToActivatePathError::PathTooShort)?
            .to_str()
            .ok_or(DeployPathToActivatePathError::InvalidUtf8)?
            .to_owned()
    ))
}

#[test]
fn test_activate_path_generation() {
    match deploy_path_to_activate_path_str(&std::path::PathBuf::from(
        "/blah/blah/deploy-rs/bin/deploy",
    )) {
        Err(_) => panic!(""),
        Ok(x) => assert_eq!(x, "/blah/blah/deploy-rs/bin/activate".to_string()),
    }
}
