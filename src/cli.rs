// SPDX-FileCopyrightText: 2020 Serokell <https://serokell.io/>
// SPDX-FileCopyrightText: 2021 Yannik Sander <contact@ysndr.de>
//
// SPDX-License-Identifier: MPL-2.0

use std::collections::HashMap;
use std::io::{stdin, stdout, Write};

use clap::{ArgMatches, Clap, FromArgMatches};

use crate as deploy;

use self::deploy::{data, settings, flake};
use log::{debug, error, info, warn};
use serde::Serialize;
use std::process::Stdio;
use thiserror::Error;
use tokio::process::Command;

/// Simple Rust rewrite of a simple Nix Flake deployment tool
#[derive(Clap, Debug, Clone)]
#[clap(version = "1.0", author = "Serokell <https://serokell.io/>")]
pub struct Opts {
    /// The flake to deploy
    #[clap(group = "deploy")]
    target: Option<String>,

    /// A list of flakes to deploy alternatively
    #[clap(long, group = "deploy")]
    targets: Option<Vec<String>>,

    /// Override hostname used for the node
    #[clap(long)]
    hostname: Option<String>,

    #[clap(flatten)]
    flags: data::Flags,

    #[clap(flatten)]
    generic_settings: settings::GenericSettings,
}

/// Returns if the available Nix installation supports flakes
async fn test_flake_support() -> Result<bool, std::io::Error> {
    debug!("Checking for flake support");

    Ok(Command::new("nix")
        .arg("eval")
        .arg("--expr")
        .arg("builtins.getFlake")
        // This will error on some machines "intentionally", and we don't really need that printing
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .await?
        .success())
}

#[derive(Serialize)]
struct PromptPart<'a> {
    user: &'a str,
    ssh_user: &'a str,
    path: &'a str,
    hostname: &'a str,
    ssh_opts: &'a [String],
}

fn print_deployment(
    parts: &[(
        &data::Target,
        data::DeployData,
        data::DeployDefs,
    )],
) -> Result<(), toml::ser::Error> {
    let mut part_map: HashMap<String, HashMap<String, PromptPart>> = HashMap::new();

    for (_, data, defs) in parts {
        part_map
            .entry(data.node_name.to_string())
            .or_insert_with(HashMap::new)
            .insert(
                data.profile_name.to_string(),
                PromptPart {
                    user: &defs.profile_user,
                    ssh_user: &defs.ssh_user,
                    path: &data.profile.profile_settings.path,
                    hostname: &data.node.node_settings.hostname,
                    ssh_opts: &data.merged_settings.ssh_opts,
                },
            );
    }

    let toml = toml::to_string(&part_map)?;

    info!("The following profiles are going to be deployed:\n{}", toml);

    Ok(())
}
#[derive(Error, Debug)]
pub enum PromptDeploymentError {
    #[error("Failed to make printable TOML of deployment: {0}")]
    TomlFormat(#[from] toml::ser::Error),
    #[error("Failed to flush stdout prior to query: {0}")]
    StdoutFlush(std::io::Error),
    #[error("Failed to read line from stdin: {0}")]
    StdinRead(std::io::Error),
    #[error("User cancelled deployment")]
    Cancelled,
}

fn prompt_deployment(
    parts: &[(
        &data::Target,
        data::DeployData,
        data::DeployDefs,
    )],
) -> Result<(), PromptDeploymentError> {
    print_deployment(parts)?;

    info!("Are you sure you want to deploy these profiles?");
    print!("> ");

    stdout()
        .flush()
        .map_err(PromptDeploymentError::StdoutFlush)?;

    let mut s = String::new();
    stdin()
        .read_line(&mut s)
        .map_err(PromptDeploymentError::StdinRead)?;

    if !yn::yes(&s) {
        if yn::is_somewhat_yes(&s) {
            info!("Sounds like you might want to continue, to be more clear please just say \"yes\". Do you want to deploy these profiles?");
            print!("> ");

            stdout()
                .flush()
                .map_err(PromptDeploymentError::StdoutFlush)?;

            let mut s = String::new();
            stdin()
                .read_line(&mut s)
                .map_err(PromptDeploymentError::StdinRead)?;

            if !yn::yes(&s) {
                return Err(PromptDeploymentError::Cancelled);
            }
        } else {
            if !yn::no(&s) {
                info!(
                    "That was unclear, but sounded like a no to me. Please say \"yes\" or \"no\" to be more clear."
                );
            }

            return Err(PromptDeploymentError::Cancelled);
        }
    }

    Ok(())
}

#[derive(Error, Debug)]
pub enum RunDeployError {
    #[error("Failed to deploy profile: {0}")]
    DeployProfile(#[from] deploy::deploy::DeployProfileError),
    #[error("Failed to push profile: {0}")]
    PushProfile(#[from] deploy::push::PushProfileError),
    #[error("No profile named `{0}` was found")]
    ProfileNotFound(String),
    #[error("No node named `{0}` was found")]
    NodeNotFound(String),
    #[error("Profile was provided without a node name")]
    ProfileWithoutNode,
    #[error("Error processing deployment definitions: {0}")]
    InvalidDeployDataDefs(#[from] data::DeployDataDefsError),
    #[error("Failed to make printable TOML of deployment: {0}")]
    TomlFormat(#[from] toml::ser::Error),
    #[error("{0}")]
    PromptDeployment(#[from] PromptDeploymentError),
    #[error("Failed to revoke profile: {0}")]
    RevokeProfile(#[from] deploy::deploy::RevokeProfileError),
}

type ToDeploy<'a> = Vec<(
    &'a data::Target,
    &'a settings::Root,
    (&'a str, &'a settings::Node),
    (&'a str, &'a settings::Profile),
)>;

async fn run_deploy(
    targets: Vec<data::Target>,
    settings: Vec<settings::Root>,
    supports_flakes: bool,
    hostname: Option<String>,
    cmd_settings: settings::GenericSettings,
    cmd_flags: data::Flags,
) -> Result<(), RunDeployError> {
    let to_deploy: ToDeploy = targets
        .iter()
        .zip(&settings)
        .map(|(target, root)| {
            let to_deploys: ToDeploy = match (&target.node, &target.profile) {
                (Some(node_name), Some(profile_name)) => {
                    let node = match root.nodes.get(node_name) {
                        Some(x) => x,
                        None => return Err(RunDeployError::NodeNotFound(node_name.clone())),
                    };
                    let profile = match node.node_settings.profiles.get(profile_name) {
                        Some(x) => x,
                        None => return Err(RunDeployError::ProfileNotFound(profile_name.clone())),
                    };

                    vec![(
                        &target,
                        &root,
                        (node_name.as_str(), node),
                        (profile_name.as_str(), profile),
                    )]
                }
                (Some(node_name), None) => {
                    let node = match root.nodes.get(node_name) {
                        Some(x) => x,
                        None => return Err(RunDeployError::NodeNotFound(node_name.clone())),
                    };

                    let mut profiles_list: Vec<(&str, &settings::Profile)> = Vec::new();

                    for profile_name in [
                        node.node_settings.profiles_order.iter().collect(),
                        node.node_settings.profiles.keys().collect::<Vec<&String>>(),
                    ]
                    .concat()
                    {
                        let profile = match node.node_settings.profiles.get(profile_name) {
                            Some(x) => x,
                            None => {
                                return Err(RunDeployError::ProfileNotFound(profile_name.clone()))
                            }
                        };

                        if !profiles_list.iter().any(|(n, _)| n == profile_name) {
                            profiles_list.push((profile_name, profile));
                        }
                    }

                    profiles_list
                        .into_iter()
                        .map(|x| (target, root, (node_name.as_str(), node), x))
                        .collect()
                }
                (None, None) => {
                    let mut l = Vec::new();

                    for (node_name, node) in &root.nodes {
                        let mut profiles_list: Vec<(&str, &settings::Profile)> = Vec::new();

                        for profile_name in [
                            node.node_settings.profiles_order.iter().collect(),
                            node.node_settings.profiles.keys().collect::<Vec<&String>>(),
                        ]
                        .concat()
                        {
                            let profile = match node.node_settings.profiles.get(profile_name) {
                                Some(x) => x,
                                None => {
                                    return Err(RunDeployError::ProfileNotFound(
                                        profile_name.clone(),
                                    ))
                                }
                            };

                            if !profiles_list.iter().any(|(n, _)| n == profile_name) {
                                profiles_list.push((profile_name, profile));
                            }
                        }

                        let ll: ToDeploy = profiles_list
                            .into_iter()
                            .map(|x| (target, root, (node_name.as_str(), node), x))
                            .collect();

                        l.extend(ll);
                    }

                    l
                }
                (None, Some(_)) => return Err(RunDeployError::ProfileWithoutNode),
            };
            Ok(to_deploys)
        })
        .collect::<Result<Vec<ToDeploy>, RunDeployError>>()?
        .into_iter()
        .flatten()
        .collect();

    let mut parts: Vec<(
        &data::Target,
        data::DeployData,
        data::DeployDefs,
    )> = Vec::new();

    for (target, root, (node_name, node), (profile_name, profile)) in to_deploy {
        let deploy_data = data::make_deploy_data(
            &root.generic_settings,
            &cmd_settings,
            &cmd_flags,
            node,
            node_name,
            profile,
            profile_name,
            hostname.as_deref(),
        );

        let deploy_defs = deploy_data.defs()?;

        parts.push((target, deploy_data, deploy_defs));
    }

    if cmd_flags.interactive {
        prompt_deployment(&parts[..])?;
    } else {
        print_deployment(&parts[..])?;
    }

    for (target, deploy_data, deploy_defs) in &parts {
        deploy::push::push_profile(deploy::push::PushProfileData {
            supports_flakes: &supports_flakes,
            check_sigs: &cmd_flags.checksigs,
            repo: &target.repo,
            deploy_data: &deploy_data,
            deploy_defs: &deploy_defs,
            keep_result: &cmd_flags.keep_result,
            result_path: cmd_flags.result_path.as_deref(),
            extra_build_args: &cmd_flags.extra_build_args,
        })
        .await?;
    }

    let mut succeeded: Vec<(&data::DeployData, &data::DeployDefs)> = vec![];

    // Run all deployments
    // In case of an error rollback any previoulsy made deployment.
    // Rollbacks adhere to the global seeting to auto_rollback and secondary
    // the profile's configuration
    for (_, deploy_data, deploy_defs) in &parts {
        if let Err(e) = deploy::deploy::deploy_profile(deploy_data, deploy_defs, cmd_flags.dry_activate).await
        {
            error!("{}", e);
            if cmd_flags.dry_activate {
                info!("dry run, not rolling back");
            }
            info!("Revoking previous deploys");
            if cmd_flags.rollback_succeeded && cmd_settings.auto_rollback.unwrap_or(true) {
                // revoking all previous deploys
                // (adheres to profile configuration if not set explicitely by
                //  the command line)
                for (deploy_data, deploy_defs) in &succeeded {
                    if deploy_data.merged_settings.auto_rollback.unwrap_or(true) {
                        deploy::deploy::revoke(*deploy_data, *deploy_defs).await?;
                    }
                }
            }
            break;
        }
        succeeded.push((deploy_data, deploy_defs))
    }

    Ok(())
}

#[derive(Error, Debug)]
pub enum RunError {
    #[error("Failed to deploy profile: {0}")]
    DeployProfile(#[from] deploy::deploy::DeployProfileError),
    #[error("Failed to push profile: {0}")]
    PushProfile(#[from] deploy::push::PushProfileError),
    #[error("Failed to test for flake support: {0}")]
    FlakeTest(std::io::Error),
    #[error("Failed to check deployment: {0}")]
    CheckDeployment(#[from] flake::CheckDeploymentError),
    #[error("Failed to evaluate deployment data: {0}")]
    GetDeploymentData(#[from] flake::GetDeploymentDataError),
    #[error("Error parsing flake: {0}")]
    ParseFlake(#[from] data::ParseTargetError),
    #[error("Error initiating logger: {0}")]
    Logger(#[from] flexi_logger::FlexiLoggerError),
    #[error("{0}")]
    RunDeploy(#[from] RunDeployError),
}

pub async fn run(args: Option<&ArgMatches>) -> Result<(), RunError> {
    let opts = match args {
        Some(o) => <Opts as FromArgMatches>::from_arg_matches(o),
        None => Opts::parse(),
    };

    deploy::init_logger(
        opts.flags.debug_logs,
        opts.flags.log_dir.as_deref(),
        &deploy::LoggerType::Deploy,
    )?;

    let deploys = opts
        .clone()
        .targets
        .unwrap_or_else(|| vec![opts.clone().target.unwrap_or_else(|| ".".to_string())]);

    let supports_flakes = test_flake_support().await.map_err(RunError::FlakeTest)?;

    if !supports_flakes {
        warn!("A Nix version without flakes support was detected, support for this is work in progress");
    }

    let targets: Vec<data::Target> = deploys
        .into_iter()
        .map(|f| f.parse::<data::Target>())
        .collect::<Result<Vec<data::Target>, data::ParseTargetError>>()?;

    if !opts.flags.skip_checks {
        for target in targets.iter() {
            flake::check_deployment(supports_flakes, &target.repo, &opts.flags.extra_build_args).await?;
        }
    }
    let settings = flake::get_deployment_data(supports_flakes, &targets, &opts.flags.extra_build_args).await?;
    run_deploy(
        targets,
        settings,
        supports_flakes,
        opts.hostname,
        opts.generic_settings,
        opts.flags,
    )
    .await?;

    Ok(())
}
