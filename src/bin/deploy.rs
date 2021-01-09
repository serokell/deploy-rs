// SPDX-FileCopyrightText: 2020 Serokell <https://serokell.io/>
//
// SPDX-License-Identifier: MPL-2.0

use std::collections::HashMap;
use std::io::{stdin, stdout, Write};

use clap::Clap;
use deploy::push::PushProfileData;

use std::process::Stdio;
use tokio::process::Command;

use thiserror::Error;

#[macro_use]
extern crate log;

#[macro_use]
extern crate serde_derive;

/// Simple Rust rewrite of a simple Nix Flake deployment tool
#[derive(Clap, Debug)]
#[clap(version = "1.0", author = "Serokell <https://serokell.io/>")]
struct Opts {
    /// The flake to deploy
    #[clap(default_value = ".")]
    flake: String,
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

    /// Override the SSH user with the given value
    #[clap(long)]
    ssh_user: Option<String>,
    /// Override the profile user with the given value
    #[clap(long)]
    profile_user: Option<String>,
    /// Override the SSH options used
    #[clap(long)]
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
    /// Where to store temporary files (only used by magic-rollback)
    #[clap(long)]
    temp_path: Option<String>,
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
enum CheckDeploymentError {
    #[error("Failed to execute Nix checking command: {0}")]
    NixCheckError(#[from] std::io::Error),
    #[error("Nix checking command resulted in a bad exit code: {0:?}")]
    NixCheckExitError(Option<i32>),
}

async fn check_deployment(
    supports_flakes: bool,
    repo: &str,
    extra_build_args: &[String],
) -> Result<(), CheckDeploymentError> {
    info!("Running checks for flake in {}", repo);

    let mut c = match supports_flakes {
        true => Command::new("nix"),
        false => Command::new("nix-build"),
    };

    let mut check_command = match supports_flakes {
        true => {
            c.arg("flake")
                .arg("check")
                .arg(repo)
        }
        false => {
            c.arg("-E")
                .arg("--no-out-link")
                .arg(format!("let r = import {}/.; x = (if builtins.isFunction r then (r {{}}) else r); in if x ? checks then x.checks.${{builtins.currentSystem}} else {{}}", repo))
        }
    };

    for extra_arg in extra_build_args {
        check_command = check_command.arg(extra_arg);
    }

    let check_status = check_command.status().await?;

    match check_status.code() {
        Some(0) => (),
        a => return Err(CheckDeploymentError::NixCheckExitError(a)),
    };

    Ok(())
}

#[derive(Error, Debug)]
enum GetDeploymentDataError {
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
}

/// Evaluates the Nix in the given `repo` and return the processed Data from it
async fn get_deployment_data(
    supports_flakes: bool,
    repo: &str,
    extra_build_args: &[String],
) -> Result<deploy::data::Data, GetDeploymentDataError> {
    info!("Evaluating flake in {}", repo);

    let mut c = match supports_flakes {
        true => Command::new("nix"),
        false => Command::new("nix-instantiate"),
    };

    let mut build_command = match supports_flakes {
        true => {
            c.arg("eval")
            .arg("--json")
            .arg(format!("{}#deploy", repo))
        }
        false => {
            c
                .arg("--strict")
                .arg("--read-write-mode")
                .arg("--json")
                .arg("--eval")
                .arg("-E")
                .arg(format!("let r = import {}/.; in if builtins.isFunction r then (r {{}}).deploy else r.deploy", repo))
        }
    };

    for extra_arg in extra_build_args {
        build_command = build_command.arg(extra_arg);
    }

    let build_child = build_command
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
    parts: &[(deploy::DeployData, deploy::DeployDefs)],
) -> Result<(), toml::ser::Error> {
    let mut part_map: HashMap<String, HashMap<String, PromptPart>> = HashMap::new();

    for (data, defs) in parts {
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
enum PromptDeploymentError {
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
    parts: &[(deploy::DeployData, deploy::DeployDefs)],
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
enum RunDeployError {
    #[error("Failed to deploy profile: {0}")]
    DeployProfileError(#[from] deploy::deploy::DeployProfileError),
    #[error("Failed to push profile: {0}")]
    PushProfileError(#[from] deploy::push::PushProfileError),
    #[error("No profile named `{0}` was found")]
    ProfileNotFound(String),
    #[error("No node named `{0}` was found")]
    NodeNotFound(String),
    #[error("Profile was provided without a node name")]
    ProfileWithoutNode,
    #[error("Error processing deployment definitions: {0}")]
    DeployDataDefsError(#[from] deploy::DeployDataDefsError),
    #[error("Failed to make printable TOML of deployment: {0}")]
    TomlFormat(#[from] toml::ser::Error),
    #[error("{0}")]
    PromptDeploymentError(#[from] PromptDeploymentError),
}

async fn run_deploy(
    deploy_flake: deploy::DeployFlake<'_>,
    data: deploy::data::Data,
    supports_flakes: bool,
    check_sigs: bool,
    interactive: bool,
    cmd_overrides: deploy::CmdOverrides,
    keep_result: bool,
    result_path: Option<&str>,
    extra_build_args: &[String],
    debug_logs: bool,
    log_dir: Option<String>,
) -> Result<(), RunDeployError> {
    let to_deploy: Vec<((&str, &deploy::data::Node), (&str, &deploy::data::Profile))> =
        match (&deploy_flake.node, &deploy_flake.profile) {
            (Some(node_name), Some(profile_name)) => {
                let node = match data.nodes.get(node_name) {
                    Some(x) => x,
                    None => return Err(RunDeployError::NodeNotFound(node_name.to_owned())),
                };
                let profile = match node.node_settings.profiles.get(profile_name) {
                    Some(x) => x,
                    None => return Err(RunDeployError::ProfileNotFound(profile_name.to_owned())),
                };

                vec![((node_name, node), (profile_name, profile))]
            }
            (Some(node_name), None) => {
                let node = match data.nodes.get(node_name) {
                    Some(x) => x,
                    None => return Err(RunDeployError::NodeNotFound(node_name.to_owned())),
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
                            return Err(RunDeployError::ProfileNotFound(profile_name.to_owned()))
                        }
                    };

                    if !profiles_list.iter().any(|(n, _)| n == profile_name) {
                        profiles_list.push((&profile_name, profile));
                    }
                }

                profiles_list
                    .into_iter()
                    .map(|x| ((node_name.as_str(), node), x))
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
                                    profile_name.to_owned(),
                                ))
                            }
                        };

                        if !profiles_list.iter().any(|(n, _)| n == profile_name) {
                            profiles_list.push((&profile_name, profile));
                        }
                    }

                    let ll: Vec<((&str, &deploy::data::Node), (&str, &deploy::data::Profile))> =
                        profiles_list
                            .into_iter()
                            .map(|x| ((node_name.as_str(), node), x))
                            .collect();

                    l.extend(ll);
                }

                l
            }
            (None, Some(_)) => return Err(RunDeployError::ProfileWithoutNode),
        };

    let mut parts: Vec<(deploy::DeployData, deploy::DeployDefs)> = Vec::new();

    for ((node_name, node), (profile_name, profile)) in to_deploy {
        let deploy_data = deploy::make_deploy_data(
            &data.generic_settings,
            node,
            node_name,
            profile,
            profile_name,
            &cmd_overrides,
            debug_logs,
            log_dir.as_deref(),
        );

        let deploy_defs = deploy_data.defs()?;

        parts.push((deploy_data, deploy_defs));
    }

    if interactive {
        prompt_deployment(&parts[..])?;
    } else {
        print_deployment(&parts[..])?;
    }

    for (deploy_data, deploy_defs) in &parts {
        deploy::push::push_profile(PushProfileData {
            supports_flakes,
            check_sigs,
            repo: deploy_flake.repo,
            deploy_data: &deploy_data,
            deploy_defs: &deploy_defs,
            keep_result,
            result_path,
            extra_build_args,
        })
        .await?;
    }

    for (deploy_data, deploy_defs) in &parts {
        deploy::deploy::deploy_profile(&deploy_data, &deploy_defs).await?;
    }

    Ok(())
}

#[derive(Error, Debug)]
enum RunError {
    #[error("Failed to deploy profile: {0}")]
    DeployProfileError(#[from] deploy::deploy::DeployProfileError),
    #[error("Failed to push profile: {0}")]
    PushProfileError(#[from] deploy::push::PushProfileError),
    #[error("Failed to test for flake support: {0}")]
    FlakeTestError(std::io::Error),
    #[error("Failed to check deployment: {0}")]
    CheckDeploymentError(#[from] CheckDeploymentError),
    #[error("Failed to evaluate deployment data: {0}")]
    GetDeploymentDataError(#[from] GetDeploymentDataError),
    #[error("Error parsing flake: {0}")]
    ParseFlakeError(#[from] deploy::ParseFlakeError),
    #[error("Error initiating logger: {0}")]
    LoggerError(#[from] flexi_logger::FlexiLoggerError),
    #[error("{0}")]
    RunDeployError(#[from] RunDeployError),
}

async fn run() -> Result<(), RunError> {
    let opts: Opts = Opts::parse();

    deploy::init_logger(
        opts.debug_logs,
        opts.log_dir.as_deref(),
        deploy::LoggerType::Deploy,
    )?;

    let deploy_flake = deploy::parse_flake(opts.flake.as_str())?;

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
    };

    let supports_flakes = test_flake_support()
        .await
        .map_err(RunError::FlakeTestError)?;

    if !supports_flakes {
        warn!("A Nix version without flakes support was detected, support for this is work in progress");
    }

    if !opts.skip_checks {
        check_deployment(supports_flakes, deploy_flake.repo, &opts.extra_build_args).await?;
    }

    let data =
        get_deployment_data(supports_flakes, deploy_flake.repo, &opts.extra_build_args).await?;

    let result_path = opts.result_path.as_deref();

    run_deploy(
        deploy_flake,
        data,
        supports_flakes,
        opts.checksigs,
        opts.interactive,
        cmd_overrides,
        opts.keep_result,
        result_path,
        &opts.extra_build_args,
        opts.debug_logs,
        opts.log_dir,
    )
    .await?;

    Ok(())
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    match run().await {
        Ok(()) => (),
        Err(err) => {
            error!("{}", err);
            std::process::exit(1);
        }
    }

    Ok(())
}
