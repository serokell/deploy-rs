// SPDX-FileCopyrightText: 2020 Serokell <https://serokell.io/>
//
// SPDX-License-Identifier: MPL-2.0

use std::borrow::Cow;
use std::path::PathBuf;

use merge::Merge;

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

pub struct CmdOverrides {
    pub ssh_user: Option<String>,
    pub profile_user: Option<String>,
    pub ssh_opts: Option<String>,
    pub fast_connection: Option<bool>,
    pub auto_rollback: Option<bool>,
    pub hostname: Option<String>,
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
            let fragment_profile_start = fragment.find('.');
            match fragment_profile_start {
                Some(s) => (Some(&fragment[..s]), Some(&fragment[s + 1..])),
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
}

pub struct DeployData<'a> {
    pub node_name: &'a str,
    pub node: &'a data::Node,
    pub profile_name: &'a str,
    pub profile: &'a data::Profile,

    pub cmd_overrides: &'a CmdOverrides,

    pub merged_settings: data::GenericSettings,
}

pub struct DeployDefs<'a> {
    pub ssh_user: Cow<'a, str>,
    pub profile_user: Cow<'a, str>,
    pub profile_path: Cow<'a, str>,
    pub current_exe: PathBuf,
    pub sudo: Option<String>,
}

impl<'a> DeployData<'a> {
    pub fn defs(&'a self) -> DeployDefs<'a> {
        let ssh_user: Cow<str> = match self.merged_settings.ssh_user {
            Some(ref u) => u.into(),
            None => whoami::username().into(),
        };

        let profile_user: Cow<str> = match self.merged_settings.user {
            Some(ref x) => x.into(),
            None => match self.merged_settings.ssh_user {
                Some(ref x) => x.into(),
                None => good_panic!(
                    "Neither user nor sshUser set for profile `{}` of node `{}`",
                    self.profile_name,
                    self.node_name
                ),
            },
        };

        let profile_path: Cow<str> = match self.profile.profile_settings.profile_path {
            None => match &profile_user[..] {
                "root" => format!("/nix/var/nix/profiles/{}", self.profile_name).into(),
                _ => format!(
                    "/nix/var/nix/profiles/per-user/{}/{}",
                    profile_user, self.profile_name
                )
                .into(),
            },
            Some(ref x) => x.into(),
        };

        let sudo: Option<String> = match self.merged_settings.user {
            Some(ref user) if user != &ssh_user => Some(format!("sudo -u {}", user)),
            _ => None,
        };

        let current_exe =
            std::env::current_exe().expect("Expected to find current executable path");

        if !current_exe.starts_with("/nix/store/") {
            good_panic!("The deploy binary must be in the Nix store");
        }

        DeployDefs {
            ssh_user,
            profile_user,
            profile_path,
            current_exe,
            sudo,
        }
    }
}

pub fn make_deploy_data<'a, 's>(
    top_settings: &'s data::GenericSettings,
    node: &'a data::Node,
    node_name: &'a str,
    profile: &'a data::Profile,
    profile_name: &'a str,
    cmd_overrides: &'a CmdOverrides,
) -> Result<DeployData<'a>, Box<dyn std::error::Error>> {
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
        merged_settings.fast_connection = fast_connection;
    }
    if let Some(auto_rollback) = cmd_overrides.auto_rollback {
        merged_settings.auto_rollback = auto_rollback;
    }

    Ok(DeployData {
        profile,
        profile_name,
        node,
        node_name,

        cmd_overrides,

        merged_settings,
    })
}

pub fn deploy_path_to_activate_path_str(
    deploy_path: &std::path::Path,
) -> Result<String, Box<dyn std::error::Error>> {
    Ok(format!(
        "{}/activate",
        deploy_path
            .parent()
            .ok_or("Deploy path too short")?
            .to_str()
            .ok_or("Deploy path is not valid utf8")?
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
