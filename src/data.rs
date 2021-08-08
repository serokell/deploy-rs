// SPDX-FileCopyrightText: 2020 Serokell <https://serokell.io/>
// SPDX-FileCopyrightText: 2021 Yannik Sander <contact@ysndr.de>
//
// SPDX-License-Identifier: MPL-2.0

use linked_hash_set::LinkedHashSet;
use rnix::{types::*, SyntaxKind::*};
use merge::Merge;
use thiserror::Error;
use clap::Clap;

use crate::settings;

#[derive(PartialEq, Debug)]
pub struct Target {
    pub repo: String,
    pub node: Option<String>,
    pub profile: Option<String>,
}

#[derive(Error, Debug)]
pub enum ParseTargetError {
    #[error("The given path was too long, did you mean to put something in quotes?")]
    PathTooLong,
    #[error("Unrecognized node or token encountered")]
    Unrecognized,
}

#[derive(Error, Debug)]
pub enum ResolveTargetError {
    #[error("No node named `{0}` was found in repo `{1}`")]
    NodeNotFound(String, String),
    #[error("No profile named `{0}` was on node `{1}` found in repo `{2}`")]
    ProfileNotFound(String, String, String),
    #[error("Profile was provided without a node name for repo `{0}`")]
    ProfileWithoutNode(String),
}

impl<'a> Target {
    pub fn resolve(
        self,
        r: &'a settings::Root,
        cs: &'a settings::GenericSettings,
        cf: &'a Flags,
        hostname: Option<&'a str>,
    ) -> Result<Vec<DeployData<'a>>, ResolveTargetError> {
        match self {
            Target{repo, node: Some(node), profile} => {
                let node_ = match r.nodes.get(&node) {
                    Some(x) => x,
                    None => return Err(ResolveTargetError::NodeNotFound(
                        node.to_owned(), repo.to_owned()
                    )),
                };
                if let Some(profile) = profile {
                    let profile_ = match node_.node_settings.profiles.get(&profile) {
                        Some(x) => x,
                        None => return Err(ResolveTargetError::ProfileNotFound(
                            profile.to_owned(), node.to_owned(), repo.to_owned()
                        )),
                    };
                    Ok({
                        let d = DeployData::new(
                            repo.to_owned(),
                            node.to_owned(),
                            profile.to_owned(),
                            &r.generic_settings,
                            cs,
                            cf,
                            node_,
                            profile_,
                            hostname,
                        );
                        vec![d]
                    })
                } else {
                    let ordered_profile_names: LinkedHashSet::<String> = node_.node_settings.profiles_order.iter().cloned().collect();
                    let profile_names: LinkedHashSet::<String> = node_.node_settings.profiles.keys().cloned().collect();
                    let prioritized_profile_names: LinkedHashSet::<&String> = ordered_profile_names.union(&profile_names).collect();
                    Ok(
                        prioritized_profile_names
                        .iter()
                        .map(
                            |p|
                            Target{repo: repo.to_owned(), node: Some(node.to_owned()), profile: Some(p.to_string())}.resolve(
                                r, cs, cf, hostname,
                            )
                        )
                        .collect::<Result<Vec<Vec<DeployData<'_>>>, ResolveTargetError>>()?
                        .into_iter().flatten().collect::<Vec<DeployData<'_>>>()
                    )
                }
            },
            Target{repo, node: None, profile: None} => {
                if let Some(hostname) = hostname {
                    todo!() // create issue to discuss:
                    // if allowed, it would be really awkward
                    // to override the hostname for a series of nodes at once
                }
                Ok(
                    r.nodes
                    .iter()
                    .map(
                        |(n, _)|
                        Target{repo: repo.to_owned(), node: Some(n.to_string()), profile: None}.resolve(
                            r, cs, cf, hostname,
                        )
                    )
                    .collect::<Result<Vec<Vec<DeployData<'_>>>, ResolveTargetError>>()?
                    .into_iter().flatten().collect::<Vec<DeployData<'_>>>()
                )
            },
            Target{repo, node: None, profile: Some(_)} => return Err(ResolveTargetError::ProfileWithoutNode(
                repo.to_owned()
            ))
        }
    }
}

impl std::str::FromStr for Target {
    type Err = ParseTargetError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let flake_fragment_start = s.find('#');
        let (repo, maybe_fragment) = match flake_fragment_start {
            Some(i) => (s[..i].to_string(), Some(&s[i + 1..])),
            None => (s.to_string(), None),
        };

        let mut node: Option<String> = None;
        let mut profile: Option<String> = None;

        if let Some(fragment) = maybe_fragment {
            let ast = rnix::parse(fragment);

            let first_child = match ast.root().node().first_child() {
                Some(x) => x,
                None => {
                    return Ok(Target {
                        repo: repo.to_owned(),
                        node: None,
                        profile: None,
                    })
                }
            };

            let mut node_over = false;

            for entry in first_child.children_with_tokens() {
                let x = match (entry.kind(), node_over) {
                    (TOKEN_DOT, false) => {
                        node_over = true;
                        None
                    }
                    (TOKEN_DOT, true) => {
                        return Err(ParseTargetError::PathTooLong);
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
                    _ => return Err(ParseTargetError::Unrecognized),
                };

                if !node_over {
                    node = x;
                } else {
                    profile = x;
                }
            }
        }

        Ok(Target {
            repo: repo.to_owned(),
            node: node,
            profile: profile,
        })
    }
}

#[test]
fn test_deploy_target_from_str() {
    assert_eq!(
        "../deploy/examples/system".parse::<Target>().unwrap(),
        Target {
            repo: "../deploy/examples/system",
            node: None,
            profile: None,
        }
    );

    assert_eq!(
        "../deploy/examples/system#".parse::<Target>().unwrap(),
        Target {
            repo: "../deploy/examples/system",
            node: None,
            profile: None,
        }
    );

    assert_eq!(
        "../deploy/examples/system#computer.\"something.nix\"".parse::<Target>().unwrap(),
        Target {
            repo: "../deploy/examples/system",
            node: Some("computer".to_string()),
            profile: Some("something.nix".to_string()),
        }
    );

    assert_eq!(
        "../deploy/examples/system#\"example.com\".system".parse::<Target>().unwrap(),
        Target {
            repo: "../deploy/examples/system",
            node: Some("example.com".to_string()),
            profile: Some("system".to_string()),
        }
    );

    assert_eq!(
        "../deploy/examples/system#example".parse::<Target>().unwrap(),
        Target {
            repo: "../deploy/examples/system",
            node: Some("example".to_string()),
            profile: None
        }
    );

    assert_eq!(
        "../deploy/examples/system#example.system".parse::<Target>().unwrap(),
        Target {
            repo: "../deploy/examples/system",
            node: Some("example".to_string()),
            profile: Some("system".to_string())
        }
    );

    assert_eq!(
        "../deploy/examples/system".parse::<Target>().unwrap(),
        Target {
            repo: "../deploy/examples/system",
            node: None,
            profile: None,
        }
    );
}

#[derive(Debug, Clone)]
pub struct DeployData<'a> {
    pub repo: String,
    pub node_name: String,
    pub profile_name: String,
    pub node: &'a settings::Node,
    pub profile: &'a settings::Profile,
    pub hostname: Option<&'a str>,

    pub flags: &'a Flags,
    pub merged_settings: settings::GenericSettings,
}

#[derive(Clap, Debug, Clone)]
pub struct Flags {
    /// Check signatures when using `nix copy`
    #[clap(short, long)]
     pub checksigs: bool,
    /// Use the interactive prompt before deployment
    #[clap(short, long)]
     pub interactive: bool,
    /// Extra arguments to be passed to nix build
     pub extra_build_args: Vec<String>,

    /// Print debug logs to output
    #[clap(short, long)]
     pub debug_logs: bool,
    /// Directory to print logs to (including the background activation process)
    #[clap(long)]
     pub log_dir: Option<String>,

    /// Keep the build outputs of each built profile
    #[clap(short, long)]
     pub keep_result: bool,
    /// Location to keep outputs from built profiles in
    #[clap(short, long)]
     pub result_path: Option<String>,

    /// Skip the automatic pre-build checks
    #[clap(short, long)]
     pub skip_checks: bool,
    /// Make activation wait for confirmation, or roll back after a period of time
    /// Show what will be activated on the machines
    #[clap(long)]
     pub dry_activate: bool,
    /// Revoke all previously succeeded deploys when deploying multiple profiles
    #[clap(long)]
     pub rollback_succeeded: bool,
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
    #[error("Value `hostname` is not define for profile {0} of node {1}")]
    NoProfileHost(String, String),
}

impl<'a> DeployData<'a> {

    fn new(
        repo: String,
        node_name: String,
        profile_name: String,
        top_settings: &'a settings::GenericSettings,
        cmd_settings: &'a settings::GenericSettings,
        flags: &'a Flags,
        node: &'a settings::Node,
        profile: &'a settings::Profile,
        hostname: Option<&'a str>,
    ) -> DeployData<'a> {
        let mut merged_settings = cmd_settings.clone();
        merged_settings.merge(profile.generic_settings.clone());
        merged_settings.merge(node.generic_settings.clone());
        merged_settings.merge(top_settings.clone());

        // if let Some(ref ssh_opts) = cmd_overrides.ssh_opts {
        //     merged_settings.ssh_opts = ssh_opts.split(' ').map(|x| x.to_owned()).collect();
        // }

        DeployData {
            repo,
            node_name,
            profile_name,
            node,
            profile,
            hostname,
            flags,
            merged_settings,
        }
    }

    pub fn defs(&'a self) -> Result<DeployDefs, DeployDataDefsError> {
        let ssh_user = match self.merged_settings.ssh_user {
            Some(ref u) => u.clone(),
            None => whoami::username(),
        };

        let profile_user = self.get_profile_user()?;

        let profile_path = self.get_profile_path()?;

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

    pub fn ssh_uri(&'a self) -> Result<String, DeployDataDefsError> {

        let hostname = match self.hostname {
            Some(x) => x,
            None => &self.node.node_settings.hostname,
        };
        let curr_user = &whoami::username();
        let ssh_user = match self.merged_settings.ssh_user {
            Some(ref u) => u,
            None => curr_user,
        };
        Ok(format!("ssh://{}@{}", ssh_user, hostname))
    }

    // can be dropped once ssh fully supports ipv6 uris
    pub fn ssh_non_uri(&'a self) -> Result<String, DeployDataDefsError> {

        let hostname = match self.hostname {
            Some(x) => x,
            None => &self.node.node_settings.hostname,
        };
        let curr_user = &whoami::username();
        let ssh_user = match self.merged_settings.ssh_user {
            Some(ref u) => u,
            None => curr_user,
        };
        Ok(format!("{}@{}", ssh_user, hostname))
    }

    pub fn ssh_opts(&'a self) -> impl Iterator<Item = &String> {
        self.merged_settings.ssh_opts.iter()
    }

    pub fn get_profile_path(&'a self) -> Result<String, DeployDataDefsError> {
        let profile_user = self.get_profile_user()?;
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
        Ok(profile_path)
    }

    pub fn get_profile_user(&'a self) -> Result<String, DeployDataDefsError> {
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
        Ok(profile_user)
    }
}
