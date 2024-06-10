// SPDX-FileCopyrightText: 2020 Serokell <https://serokell.io/>
// SPDX-FileCopyrightText: 2021 Yannik Sander <contact@ysndr.de>
//
// SPDX-License-Identifier: MPL-2.0

use std::collections::HashMap;
use std::io::{stdin, stdout, Write};

use clap::{ArgMatches, Clap, FromArgMatches};
use futures_util::future::{join_all, try_join_all};
use tokio::try_join;

use crate as deploy;
use crate::push::{PushProfileData, PushProfileError};

use self::deploy::{DeployFlake, ParseFlakeError};
use futures_util::stream::{StreamExt, TryStreamExt};
use log::{debug, error, info, warn};
use serde::Serialize;
use std::path::PathBuf;
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
    /// Check signatures when using `nix copy`
    #[clap(short, long)]
    checksigs: bool,
    /// Use the interactive prompt before deployment
    #[clap(short, long)]
    interactive: bool,
    /// Extra arguments to be passed to nix build
    extra_build_args: Vec<String>,

    /// Print debug logs to output
    #[clap(short, long)]
    debug_logs: bool,
    /// Directory to print logs to (including the background activation process)
    #[clap(long)]
    log_dir: Option<String>,

    /// Keep the build outputs of each built profile
    #[clap(short, long)]
    keep_result: bool,
    /// Location to keep outputs from built profiles in
    #[clap(short, long)]
    result_path: Option<String>,

    /// Skip the automatic pre-build checks
    #[clap(short, long)]
    skip_checks: bool,

    /// Build on remote host
    #[clap(long)]
    remote_build: bool,

    /// Override the SSH user with the given value
    #[clap(long)]
    ssh_user: Option<String>,
    /// Override the profile user with the given value
    #[clap(long)]
    profile_user: Option<String>,
    /// Override the SSH options used
    #[clap(long, allow_hyphen_values = true)]
    ssh_opts: Option<String>,
    /// Override if the connecting to the target node should be considered fast
    #[clap(long)]
    fast_connection: Option<bool>,
    /// Override if a rollback should be attempted if activation fails
    #[clap(long)]
    auto_rollback: Option<bool>,
    /// Override hostname used for the node
    #[clap(long)]
    hostname: Option<String>,
    /// Make activation wait for confirmation, or roll back after a period of time
    #[clap(long)]
    magic_rollback: Option<bool>,
    /// How long activation should wait for confirmation (if using magic-rollback)
    #[clap(long)]
    confirm_timeout: Option<u16>,
    /// How long we should wait for profile activation
    #[clap(long)]
    activation_timeout: Option<u16>,
    /// Where to store temporary files (only used by magic-rollback)
    #[clap(long)]
    temp_path: Option<PathBuf>,
    /// Show what will be activated on the machines
    #[clap(long)]
    dry_activate: bool,
    /// Don't activate, but update the boot loader to boot into the new profile
    #[clap(long)]
    boot: bool,
    /// Revoke all previously succeeded deploys when deploying multiple profiles
    #[clap(long)]
    rollback_succeeded: Option<bool>,
    /// Which sudo command to use. Must accept at least two arguments: user name to execute commands as and the rest is the command to execute
    #[clap(long)]
    sudo: Option<String>,
    /// Prompt for sudo password during activation.
    #[clap(long)]
    interactive_sudo: Option<bool>,
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

#[derive(Error, Debug)]
pub enum CheckDeploymentError {
    #[error("Failed to execute Nix checking command: {0}")]
    NixCheck(#[from] std::io::Error),
    #[error("Nix checking command resulted in a bad exit code: {0:?}")]
    NixCheckExit(Option<i32>),
}

async fn check_deployment(
    supports_flakes: bool,
    repo: &str,
    extra_build_args: &[String],
) -> Result<(), CheckDeploymentError> {
    info!("Running checks for flake in {}", repo);

    let mut check_command = match supports_flakes {
        true => Command::new("nix"),
        false => Command::new("nix-build"),
    };

    if supports_flakes {
        check_command.arg("flake").arg("check").arg(repo);
    } else {
        check_command.arg("-E")
                .arg("--no-out-link")
                .arg(format!("let r = import {}/.; x = (if builtins.isFunction r then (r {{}}) else r); in if x ? checks then x.checks.${{builtins.currentSystem}} else {{}}", repo));
    }

    check_command.args(extra_build_args);

    let check_status = check_command.status().await?;

    match check_status.code() {
        Some(0) => (),
        a => return Err(CheckDeploymentError::NixCheckExit(a)),
    };

    Ok(())
}

#[derive(Error, Debug)]
pub enum GetDeploymentDataError {
    #[error("Failed to execute nix eval command: {0}")]
    NixEval(std::io::Error),
    #[error("Failed to read output from evaluation: {0}")]
    NixEvalOut(std::io::Error),
    #[error("Evaluation resulted in a bad exit code: {0:?}")]
    NixEvalExit(Option<i32>),
    #[error("Error converting evaluation output to utf8: {0}")]
    DecodeUtf8(#[from] std::string::FromUtf8Error),
    #[error("Error decoding the JSON from evaluation: {0}")]
    DecodeJson(#[from] serde_json::error::Error),
    #[error("Impossible happened: profile is set but node is not")]
    ProfileNoNode,
}

/// Evaluates the Nix in the given `repo` and return the processed Data from it
async fn get_deployment_data(
    supports_flakes: bool,
    flakes: &[deploy::DeployFlake<'_>],
    extra_build_args: &[String],
) -> Result<Vec<deploy::data::Data>, GetDeploymentDataError> {
    futures_util::stream::iter(flakes).then(|flake| async move {

    info!("Evaluating flake in {}", flake.repo);

    let mut c = if supports_flakes {
        Command::new("nix")
    } else {
        Command::new("nix-instantiate")
    };

    if supports_flakes {
        c.arg("eval")
            .arg("--json")
            .arg(format!("{}#deploy", flake.repo))
            // We use --apply instead of --expr so that we don't have to deal with builtins.getFlake
            .arg("--apply");
        match (&flake.node, &flake.profile) {
            (Some(node), Some(profile)) => {
                // Ignore all nodes and all profiles but the one we're evaluating
                c.arg(format!(
                    r#"
                      deploy:
                      (deploy // {{
                        nodes = {{
                          "{0}" = deploy.nodes."{0}" // {{
                            profiles = {{
                              inherit (deploy.nodes."{0}".profiles) "{1}";
                            }};
                          }};
                        }};
                      }})
                     "#,
                    node, profile
                ))
            }
            (Some(node), None) => {
                // Ignore all nodes but the one we're evaluating
                c.arg(format!(
                    r#"
                      deploy:
                      (deploy // {{
                        nodes = {{
                          inherit (deploy.nodes) "{}";
                        }};
                      }})
                    "#,
                    node
                ))
            }
            (None, None) => {
                // We need to evaluate all profiles of all nodes anyway, so just do it strictly
                c.arg("deploy: deploy")
            }
            (None, Some(_)) => return Err(GetDeploymentDataError::ProfileNoNode),
        }
    } else {
        c
            .arg("--strict")
            .arg("--read-write-mode")
            .arg("--json")
            .arg("--eval")
            .arg("-E")
            .arg(format!("let r = import {}/.; in if builtins.isFunction r then (r {{}}).deploy else r.deploy", flake.repo))
    };

    c.args(extra_build_args);

    let build_child = c
        .stdout(Stdio::piped())
        .spawn()
        .map_err(GetDeploymentDataError::NixEval)?;

    let build_output = build_child
        .wait_with_output()
        .await
        .map_err(GetDeploymentDataError::NixEvalOut)?;

    match build_output.status.code() {
        Some(0) => (),
        a => return Err(GetDeploymentDataError::NixEvalExit(a)),
    };

    let data_json = String::from_utf8(build_output.stdout)?;

    Ok(serde_json::from_str(&data_json)?)
}).try_collect().await
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
        &deploy::DeployFlake<'_>,
        deploy::DeployData,
        deploy::DeployDefs,
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
        &deploy::DeployFlake<'_>,
        deploy::DeployData,
        deploy::DeployDefs,
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
    DeployDataDefs(#[from] deploy::DeployDataDefsError),
    #[error("Failed to make printable TOML of deployment: {0}")]
    TomlFormat(#[from] toml::ser::Error),
    #[error("{0}")]
    PromptDeployment(#[from] PromptDeploymentError),
    #[error("Failed to revoke profile: {0}")]
    RevokeProfile(#[from] deploy::deploy::RevokeProfileError),
    #[error("Deployment failed, rolled back to previous generation")]
    Rollback
}

type ToDeploy<'a> = Vec<(
    &'a deploy::DeployFlake<'a>,
    &'a deploy::data::Data,
    (&'a str, &'a deploy::data::Node),
    (&'a str, &'a deploy::data::Profile),
)>;

async fn run_deploy(
    deploy_flakes: Vec<deploy::DeployFlake<'_>>,
    data: Vec<deploy::data::Data>,
    supports_flakes: bool,
    check_sigs: bool,
    interactive: bool,
    cmd_overrides: &deploy::CmdOverrides,
    keep_result: bool,
    result_path: Option<&str>,
    extra_build_args: &[String],
    debug_logs: bool,
    dry_activate: bool,
    boot: bool,
    log_dir: &Option<String>,
    rollback_succeeded: bool,
) -> Result<(), RunDeployError> {
    let to_deploy: ToDeploy = deploy_flakes
        .iter()
        .zip(&data)
        .map(|(deploy_flake, data)| {
            let to_deploys: ToDeploy = match (&deploy_flake.node, &deploy_flake.profile) {
                (Some(node_name), Some(profile_name)) => {
                    let node = match data.nodes.get(node_name) {
                        Some(x) => x,
                        None => return Err(RunDeployError::NodeNotFound(node_name.clone())),
                    };
                    let profile = match node.node_settings.profiles.get(profile_name) {
                        Some(x) => x,
                        None => return Err(RunDeployError::ProfileNotFound(profile_name.clone())),
                    };

                    vec![(
                        deploy_flake,
                        data,
                        (node_name.as_str(), node),
                        (profile_name.as_str(), profile),
                    )]
                }
                (Some(node_name), None) => {
                    let node = match data.nodes.get(node_name) {
                        Some(x) => x,
                        None => return Err(RunDeployError::NodeNotFound(node_name.clone())),
                    };

                    let mut profiles_list: Vec<(&str, &deploy::data::Profile)> = Vec::new();

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
                        .map(|x| (deploy_flake, data, (node_name.as_str(), node), x))
                        .collect()
                }
                (None, None) => {
                    let mut l = Vec::new();

                    for (node_name, node) in &data.nodes {
                        let mut profiles_list: Vec<(&str, &deploy::data::Profile)> = Vec::new();

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
                            .map(|x| (deploy_flake, data, (node_name.as_str(), node), x))
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
        &deploy::DeployFlake<'_>,
        deploy::DeployData,
        deploy::DeployDefs,
    )> = Vec::new();

    for (deploy_flake, data, (node_name, node), (profile_name, profile)) in to_deploy {
        let deploy_data = deploy::make_deploy_data(
            &data.generic_settings,
            node,
            node_name,
            profile,
            profile_name,
            cmd_overrides,
            debug_logs,
            log_dir.as_deref(),
        );

        let mut deploy_defs = deploy_data.defs()?;

        if deploy_data.merged_settings.interactive_sudo.unwrap_or(false) {
            warn!("Interactive sudo is enabled! Using a sudo password is less secure than correctly configured SSH keys.\nPlease use keys in production environments.");

            if deploy_data.merged_settings.sudo.is_some() {
                warn!("Custom sudo commands should be configured to accept password input from stdin when using the 'interactive sudo' option. Deployment may fail if the custom command ignores stdin.");
            } else {
                // this configures sudo to hide the password prompt and accept input from stdin
                // at the time of writing, deploy_defs.sudo defaults to 'sudo -u root' when using user=root and sshUser as non-root
                let original = deploy_defs.sudo.unwrap_or("sudo".to_string());
                deploy_defs.sudo = Some(format!("{} -S -p \"\"", original));
            }

            info!("You will now be prompted for the sudo password for {}.", node.node_settings.hostname);
            let sudo_password = rpassword::prompt_password(format!("(sudo for {}) Password: ", node.node_settings.hostname)).unwrap_or("".to_string());

            deploy_defs.sudo_password = Some(sudo_password);
        }

        parts.push((deploy_flake, deploy_data, deploy_defs));
    }

    if interactive {
        prompt_deployment(&parts[..])?;
    } else {
        print_deployment(&parts[..])?;
    }

    let data_iter = || {
        parts.iter().map(
            |(deploy_flake, deploy_data, deploy_defs)| deploy::push::PushProfileData {
                supports_flakes,
                check_sigs,
                repo: deploy_flake.repo,
                deploy_data,
                deploy_defs,
                keep_result,
                result_path,
                extra_build_args,
            },
        )
    };

    let (remote_builds, local_builds): (Vec<_>, Vec<_>) = data_iter().partition(|data| {
        data.deploy_data.merged_settings.remote_build.unwrap_or_default()
    });

    // the grouping by host will retain each hosts ordering by profiles_order since the fold is synchronous
    let remote_build_map: HashMap<_, Vec<_>> = remote_builds.iter().fold(HashMap::new(), |mut accum, elem| {
        match accum.get_mut(elem.deploy_data.node_name) {
            Some(v) => { v.push(elem); accum },
            None => { accum.insert(elem.deploy_data.node_name, vec![elem]); accum }
        }
    });

    try_join!(
        // remote builds can be run asynchronously (per host)
        try_join_all(remote_build_map.into_iter().map(deploy_profiles_to_host)),
        async {
            // run local builds synchronously to prevent hardware deadlocks
            for data in &local_builds {
                deploy::push::build_profile(data).await.unwrap();
            }

            // push all profiles asynchronously
            join_all(local_builds.into_iter().map(|data| async {
                let data = data;
                deploy::push::push_profile(&data).await
            })).await;

            Ok(())
        }
    )?;


    let mut succeeded: Vec<(&deploy::DeployData, &deploy::DeployDefs)> = vec![];

    // Run all deployments
    // In case of an error rollback any previoulsy made deployment.
    // Rollbacks adhere to the global seeting to auto_rollback and secondary
    // the profile's configuration
    for (_, deploy_data, deploy_defs) in &parts {
        if let Err(e) = deploy::deploy::deploy_profile(deploy_data, deploy_defs, dry_activate, boot).await
        {
            error!("{}", e);
            if dry_activate {
                info!("dry run, not rolling back");
            }
            if rollback_succeeded && cmd_overrides.auto_rollback.unwrap_or(true) {
                info!("Revoking previous deploys");
                // revoking all previous deploys
                // (adheres to profile configuration if not set explicitely by
                //  the command line)
                for (deploy_data, deploy_defs) in &succeeded {
                    if deploy_data.merged_settings.auto_rollback.unwrap_or(true) {
                        deploy::deploy::revoke(*deploy_data, *deploy_defs).await?;
                    }
                }
                return Err(RunDeployError::Rollback);
            }
            return Err(RunDeployError::DeployProfile(e))
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
    CheckDeployment(#[from] CheckDeploymentError),
    #[error("Failed to evaluate deployment data: {0}")]
    GetDeploymentData(#[from] GetDeploymentDataError),
    #[error("Error parsing flake: {0}")]
    ParseFlake(#[from] deploy::ParseFlakeError),
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
        opts.debug_logs,
        opts.log_dir.as_deref(),
        &deploy::LoggerType::Deploy,
    )?;

    if opts.dry_activate && opts.boot {
        error!("Cannot use both --dry-activate & --boot!");
    }

    let deploys = opts
        .clone()
        .targets
        .unwrap_or_else(|| vec![opts.clone().target.unwrap_or_else(|| ".".to_string())]);

    let deploy_flakes: Vec<DeployFlake> = deploys
        .iter()
        .map(|f| deploy::parse_flake(f.as_str()))
        .collect::<Result<Vec<DeployFlake>, ParseFlakeError>>()?;

    let cmd_overrides = deploy::CmdOverrides {
        ssh_user: opts.ssh_user,
        profile_user: opts.profile_user,
        ssh_opts: opts.ssh_opts,
        fast_connection: opts.fast_connection,
        auto_rollback: opts.auto_rollback,
        hostname: opts.hostname,
        magic_rollback: opts.magic_rollback,
        temp_path: opts.temp_path,
        confirm_timeout: opts.confirm_timeout,
        activation_timeout: opts.activation_timeout,
        dry_activate: opts.dry_activate,
        remote_build: opts.remote_build,
        sudo: opts.sudo,
        interactive_sudo: opts.interactive_sudo
    };

    let supports_flakes = test_flake_support().await.map_err(RunError::FlakeTest)?;

    if !supports_flakes {
        warn!("A Nix version without flakes support was detected, support for this is work in progress");
    }

    if !opts.skip_checks {
        for deploy_flake in &deploy_flakes {
            check_deployment(supports_flakes, deploy_flake.repo, &opts.extra_build_args).await?;
        }
    }
    let result_path = opts.result_path.as_deref();
    let data = get_deployment_data(supports_flakes, &deploy_flakes, &opts.extra_build_args).await?;
    run_deploy(
        deploy_flakes,
        data,
        supports_flakes,
        opts.checksigs,
        opts.interactive,
        &cmd_overrides,
        opts.keep_result,
        result_path,
        &opts.extra_build_args,
        opts.debug_logs,
        opts.dry_activate,
        opts.boot,
        &opts.log_dir,
        opts.rollback_succeeded.unwrap_or(true),
    )
    .await?;

    Ok(())
}

async fn deploy_profiles_to_host<'a>((_host, profiles): (&str, Vec<&'a PushProfileData<'a>>)) -> Result<(), PushProfileError> {
    for profile in &profiles {
        deploy::push::build_profile(profile).await?;
    };
    Ok(())
}
