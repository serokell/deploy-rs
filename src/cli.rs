// SPDX-FileCopyrightText: 2020 Serokell <https://serokell.io/>
// SPDX-FileCopyrightText: 2021 Yannik Sander <contact@ysndr.de>
//
// SPDX-License-Identifier: MPL-2.0

use std::collections::HashMap;
use std::io::{stdin, stdout, Write};

use clap::Parser;

use crate as deploy;

use self::deploy::{data, flake, settings};
use log::{debug, error, info, warn};
use serde::Serialize;
use std::env;
use std::process::Stdio;
use thiserror::Error;
use tokio::process::Command;

use std::path::{Path, PathBuf};

/// Simple Rust rewrite of a simple Nix Flake deployment tool
#[derive(Parser, Debug, Clone, Default)]
#[clap(version = "1.0", author = "Serokell <https://serokell.io/>")]
pub struct Opts {
    /// The flake to deploy
    #[clap(group = "deploy")]
    pub target: Option<String>,

    /// A list of flakes to deploy alternatively
    #[clap(long, group = "deploy")]
    pub targets: Option<Vec<String>>,

    /// Override hostname used for the node
    #[clap(long)]
    pub hostname: Option<String>,

    #[clap(flatten)]
    pub flags: data::Flags,

    #[clap(flatten)]
    pub generic_settings: settings::GenericSettings,
}

/// Returns if the available Nix installation supports flakes
async fn test_flake_support() -> Result<bool, std::io::Error> {
    debug!("Checking for flake support");

    Ok(Command::new("nix")
        .args(vec!["eval", "--expr", "builtins.getFlake"])
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
    path: &'a str,
    uri: &'a str,
    ssh_opts: &'a [String],
}

fn print_deployment(parts: &[&data::DeployData]) -> Result<(), toml::ser::Error> {
    let mut part_map: HashMap<String, HashMap<String, PromptPart>> = HashMap::new();

    for data in parts {
        part_map
            .entry(data.node_name.to_string())
            .or_insert_with(HashMap::new)
            .insert(
                data.profile_name.to_string(),
                PromptPart {
                    user: &data.profile_user,
                    path: &data.profile.profile_settings.path,
                    uri: &data.ssh_uri,
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

fn prompt_deployment(parts: &[&data::DeployData]) -> Result<(), PromptDeploymentError> {
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
    #[error("Failed to resolve target: {0}")]
    ResolveTarget(#[from] data::ResolveTargetError),
    #[error("Failed run Nix")]
    Nix(#[from] std::io::Error),
    #[error("Failed to parse JSON")]
    JSON(#[from] serde_json::Error),
    #[error("Error processing deployment definitions: {0}")]
    DeployData(#[from] data::DeployDataError),
    #[error("Failed to make printable TOML of deployment: {0}")]
    TomlFormat(#[from] toml::ser::Error),
    #[error("{0}")]
    PromptDeployment(#[from] PromptDeploymentError),
    #[error("Failed to revoke profile: {0}")]
    RevokeProfile(#[from] deploy::deploy::RevokeProfileError),
}

fn find_flake(starting_directory: &Path) -> Option<PathBuf> {
    let mut path: PathBuf = starting_directory.into();
    let file = Path::new("flake.nix");

    loop {
        path.push(file);

        if path.is_file() {
            break Some(path);
        }

        if !(path.pop() && path.pop()) {
            // remove file && remove parent
            break None;
        }
    }
}
async fn run_deploy(
    targets: Vec<data::Target>,
    settings: Vec<settings::Root>,
    supports_flakes: bool,
    hostname: Option<String>,
    cmd_settings: settings::GenericSettings,
    cmd_flags: data::Flags,
) -> Result<(), RunDeployError> {
    if supports_flakes {
        let path = find_flake(Path::new(&env::current_dir()?)).unwrap_or_default();
        let flake = path.to_str().unwrap_or_default();
        let config_cmd = Command::new("nix")
            .args(vec!["eval", "--raw", "--impure", "--expr"])
            .arg(format!(
                "let flake = import {}; in if flake ? nixConfig then flake.nixConfig else {}",
                flake, "{}"
            ))
            .arg("--apply")
            .arg(include_str!("../lib/nix_config.nix"))
            .output()
            .await?;
        if config_cmd.status.success() {
            env::set_var("NIX_CONFIG", &*String::from_utf8_lossy(&config_cmd.stdout));
        }
    }
    let deploy_datas_ = targets
        .into_iter()
        .zip(&settings)
        .map(|(target, root)| target.resolve(root, &cmd_settings, &cmd_flags, hostname.to_owned()))
        .collect::<Result<Vec<Vec<data::DeployData<'_>>>, data::ResolveTargetError>>()?;
    let deploy_datas: Vec<&data::DeployData<'_>> = deploy_datas_.iter().flatten().collect();

    let mut parts: Vec<&data::DeployData> = Vec::new();

    for deploy_data in deploy_datas {
        parts.push(deploy_data);
    }

    if cmd_flags.interactive {
        prompt_deployment(&parts[..])?;
    } else {
        print_deployment(&parts[..])?;
    }

    for deploy_data in &parts {
        deploy::push::push_profile(
            supports_flakes,
            deploy::push::ShowDerivationCommand::from_data(deploy_data),
            deploy::push::BuildCommand::from_data(deploy_data),
            deploy::push::SignCommand::from_data(deploy_data),
            deploy::push::CopyCommand::from_data(deploy_data),
        )
        .await?;
    }

    let mut succeeded: Vec<&data::DeployData> = vec![];

    // Run all deployments
    // In case of an error rollback any previoulsy made deployment.
    // Rollbacks adhere to the global seeting to auto_rollback and secondary
    // the profile's configuration
    for deploy_data in &parts {
        if let Err(e) = deploy::deploy::deploy_profile(
            &deploy_data.node_name,
            &deploy_data.profile_name,
            deploy::deploy::SshCommand::from_data(deploy_data)?,
            deploy::deploy::ActivateCommand::from_data(deploy_data),
            deploy::deploy::WaitCommand::from_data(deploy_data),
            deploy::deploy::ConfirmCommand::from_data(deploy_data),
        )
        .await
        {
            error!("{}", e);
            if cmd_flags.dry_activate {
                info!("dry run, not rolling back");
            }
            if cmd_flags.rollback_succeeded && cmd_settings.auto_rollback {
                info!("Revoking previous deploys");
                // revoking all previous deploys
                // (adheres to profile configuration if not set explicitely by
                //  the command line)
                for deploy_data in &succeeded {
                    if deploy_data.merged_settings.auto_rollback {
                        deploy::deploy::revoke(
                            &deploy_data.node_name,
                            &deploy_data.profile_name,
                            deploy::deploy::SshCommand::from_data(deploy_data)?,
                            deploy::deploy::RevokeCommand::from_data(deploy_data),
                        )
                        .await?;
                    }
                }
            }
            break;
        }
        succeeded.push(deploy_data)
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

pub async fn run(args: Option<Opts>) -> Result<(), RunError> {
    let opts = match args {
        Some(o) => o,
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
        .collect::<Result<Vec<data::Target>, data::ParseTargetError>>(
    )?;

    if opts.flags.do_checks {
        for target in targets.iter() {
            flake::check_deployment(supports_flakes, &target.repo, &opts.flags.extra_build_args)
                .await?;
        }
    }
    let settings =
        flake::get_deployment_data(supports_flakes, &targets, &opts.flags.extra_build_args).await?;
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
