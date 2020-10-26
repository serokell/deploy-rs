// SPDX-FileCopyrightText: 2020 Serokell <https://serokell.io/>
//
// SPDX-License-Identifier: MPL-2.0

use clap::Clap;

use std::process::Stdio;
use tokio::process::Command;

use merge::Merge;

extern crate pretty_env_logger;

#[macro_use]
extern crate log;

#[macro_use]
extern crate serde_derive;

#[macro_use]
mod utils;

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
    /// Extra arguments to be passed to nix build
    extra_build_args: Vec<String>,

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

#[inline]
async fn push_all_profiles(
    node: &utils::data::Node,
    node_name: &str,
    supports_flakes: bool,
    repo: &str,
    top_settings: &utils::data::GenericSettings,
    check_sigs: bool,
    cmd_overrides: &utils::CmdOverrides,
    keep_result: bool,
    result_path: Option<&str>,
) -> Result<(), Box<dyn std::error::Error>> {
    info!("Pushing all profiles for `{}`", node_name);

    let mut profiles_list: Vec<&str> = node
        .node_settings
        .profiles_order
        .iter()
        .map(|x| x.as_ref())
        .collect();

    // Add any profiles which weren't in the provided order list
    for profile_name in node.node_settings.profiles.keys() {
        if !profiles_list.contains(&profile_name.as_str()) {
            profiles_list.push(&profile_name);
        }
    }

    for profile_name in profiles_list {
        let profile = match node.node_settings.profiles.get(profile_name) {
            Some(x) => x,
            None => good_panic!("No profile was found named `{}`", profile_name),
        };

        let mut merged_settings = top_settings.clone();
        merged_settings.merge(node.generic_settings.clone());
        merged_settings.merge(profile.generic_settings.clone());

        let deploy_data = utils::make_deploy_data(
            top_settings,
            node,
            node_name,
            profile,
            profile_name,
            cmd_overrides,
        )?;

        let deploy_defs = deploy_data.defs();

        utils::push::push_profile(
            supports_flakes,
            check_sigs,
            repo,
            &deploy_data,
            &deploy_defs,
            keep_result,
            result_path,
        )
        .await?;
    }

    Ok(())
}

#[inline]
async fn deploy_all_profiles(
    node: &utils::data::Node,
    node_name: &str,
    top_settings: &utils::data::GenericSettings,
    cmd_overrides: &utils::CmdOverrides,
) -> Result<(), Box<dyn std::error::Error>> {
    info!("Deploying all profiles for `{}`", node_name);

    let mut profiles_list: Vec<&str> = node
        .node_settings
        .profiles_order
        .iter()
        .map(|x| x.as_ref())
        .collect();

    // Add any profiles which weren't in the provided order list
    for profile_name in node.node_settings.profiles.keys() {
        if !profiles_list.contains(&profile_name.as_str()) {
            profiles_list.push(&profile_name);
        }
    }

    for profile_name in profiles_list {
        let profile = match node.node_settings.profiles.get(profile_name) {
            Some(x) => x,
            None => good_panic!("No profile was found named `{}`", profile_name),
        };

        let mut merged_settings = top_settings.clone();
        merged_settings.merge(node.generic_settings.clone());
        merged_settings.merge(profile.generic_settings.clone());

        let deploy_data = utils::make_deploy_data(
            top_settings,
            node,
            node_name,
            profile,
            profile_name,
            cmd_overrides,
        )?;

        let deploy_defs = deploy_data.defs();

        utils::deploy::deploy_profile(&deploy_data, &deploy_defs).await?;
    }

    Ok(())
}

/// Returns if the available Nix installation supports flakes
#[inline]
async fn test_flake_support() -> Result<bool, Box<dyn std::error::Error>> {
    Ok(Command::new("nix")
        .arg("eval")
        .arg("--expr")
        .arg("builtins.getFlake")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .await?
        .success())
}

async fn check_deployment(supports_flakes: bool, repo: &str, extra_build_args: &[String]) -> () {
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
                .arg(format!("let r = import {}/.; in (if builtins.isFunction r then (r {{}}) else r).checks.${{builtins.currentSystem}}", repo))
        }
    };

    for extra_arg in extra_build_args {
        check_command = check_command.arg(extra_arg);
    }

    let check_status = match check_command.status().await {
        Ok(x) => x,
        Err(err) => good_panic!("Error running checks for the given flake repo: {:?}", err),
    };

    if !check_status.success() {
        good_panic!("Checks failed for the given flake repo");
    }

    ()
}

/// Evaluates the Nix in the given `repo` and return the processed Data from it
async fn get_deployment_data(
    supports_flakes: bool,
    repo: &str,
    extra_build_args: &[String],
) -> Result<utils::data::Data, Box<dyn std::error::Error>> {
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

    let build_child = build_command.stdout(Stdio::piped()).spawn()?;

    let build_output = build_child.wait_with_output().await?;

    if !build_output.status.success() {
        good_panic!(
            "Error building deploy props for the provided flake: {}",
            repo
        );
    }

    let data_json = String::from_utf8(build_output.stdout)?;

    Ok(serde_json::from_str(&data_json)?)
}

async fn run_deploy(
    deploy_flake: utils::DeployFlake<'_>,
    data: utils::data::Data,
    supports_flakes: bool,
    check_sigs: bool,
    cmd_overrides: utils::CmdOverrides,
    keep_result: bool,
    result_path: Option<&str>,
) -> Result<(), Box<dyn std::error::Error>> {
    match (deploy_flake.node, deploy_flake.profile) {
        (Some(node_name), Some(profile_name)) => {
            let node = match data.nodes.get(node_name) {
                Some(x) => x,
                None => good_panic!("No node was found named `{}`", node_name),
            };
            let profile = match node.node_settings.profiles.get(profile_name) {
                Some(x) => x,
                None => good_panic!("No profile was found named `{}`", profile_name),
            };

            let deploy_data = utils::make_deploy_data(
                &data.generic_settings,
                node,
                node_name,
                profile,
                profile_name,
                &cmd_overrides,
            )?;

            let deploy_defs = deploy_data.defs();

            utils::push::push_profile(
                supports_flakes,
                check_sigs,
                deploy_flake.repo,
                &deploy_data,
                &deploy_defs,
                keep_result,
                result_path,
            )
            .await?;

            utils::deploy::deploy_profile(&deploy_data, &deploy_defs).await?;
        }
        (Some(node_name), None) => {
            let node = match data.nodes.get(node_name) {
                Some(x) => x,
                None => good_panic!("No node was found named `{}`", node_name),
            };

            push_all_profiles(
                node,
                node_name,
                supports_flakes,
                deploy_flake.repo,
                &data.generic_settings,
                check_sigs,
                &cmd_overrides,
                keep_result,
                result_path,
            )
            .await?;

            deploy_all_profiles(node, node_name, &data.generic_settings, &cmd_overrides).await?;
        }
        (None, None) => {
            info!("Deploying all profiles on all nodes");

            for (node_name, node) in &data.nodes {
                push_all_profiles(
                    node,
                    node_name,
                    supports_flakes,
                    deploy_flake.repo,
                    &data.generic_settings,
                    check_sigs,
                    &cmd_overrides,
                    keep_result,
                    result_path,
                )
                .await?;
            }

            for (node_name, node) in &data.nodes {
                deploy_all_profiles(node, node_name, &data.generic_settings, &cmd_overrides)
                    .await?;
            }
        }
        (None, Some(_)) => {
            good_panic!("Profile provided without a node, this is not (currently) supported")
        }
    };

    Ok(())
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    if std::env::var("DEPLOY_LOG").is_err() {
        std::env::set_var("DEPLOY_LOG", "info");
    }

    pretty_env_logger::init_custom_env("DEPLOY_LOG");

    let opts: Opts = Opts::parse();

    let deploy_flake = utils::parse_flake(opts.flake.as_str());

    let cmd_overrides = utils::CmdOverrides {
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

    let supports_flakes = test_flake_support().await?;

    if !supports_flakes {
        warn!("A Nix version without flakes support was detected, support for this is work in progress");
    }

    if !opts.skip_checks {
        check_deployment(supports_flakes, deploy_flake.repo, &opts.extra_build_args).await;
    }

    let data =
        get_deployment_data(supports_flakes, deploy_flake.repo, &opts.extra_build_args).await?;

    let result_path = opts.result_path.as_deref();

    run_deploy(
        deploy_flake,
        data,
        supports_flakes,
        opts.checksigs,
        cmd_overrides,
        opts.keep_result,
        result_path,
    )
    .await?;

    Ok(())
}
