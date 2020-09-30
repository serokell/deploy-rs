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
}

#[inline]
async fn push_all_profiles(
    node: &utils::data::Node,
    node_name: &str,
    supports_flakes: bool,
    repo: &str,
    top_settings: &utils::data::GenericSettings,
    check_sigs: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    info!("Pushing all profiles for `{}`", node_name);

    let mut profiles_list: Vec<&str> = node.profiles_order.iter().map(|x| x.as_ref()).collect();

    // Add any profiles which weren't in the provided order list
    for profile_name in node.profiles.keys() {
        if !profiles_list.contains(&profile_name.as_str()) {
            profiles_list.push(&profile_name);
        }
    }

    for profile_name in profiles_list {
        let profile = match node.profiles.get(profile_name) {
            Some(x) => x,
            None => good_panic!("No profile was found named `{}`", profile_name),
        };

        let mut merged_settings = top_settings.clone();
        merged_settings.merge(node.generic_settings.clone());
        merged_settings.merge(profile.generic_settings.clone());

        let deploy_data =
            utils::make_deploy_data(profile_name, node_name, &merged_settings).await?;

        utils::push::push_profile(
            profile,
            profile_name,
            node,
            node_name,
            supports_flakes,
            check_sigs,
            repo,
            &merged_settings,
            &deploy_data,
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
) -> Result<(), Box<dyn std::error::Error>> {
    info!("Deploying all profiles for `{}`", node_name);

    let mut profiles_list: Vec<&str> = node.profiles_order.iter().map(|x| x.as_ref()).collect();

    // Add any profiles which weren't in the provided order list
    for profile_name in node.profiles.keys() {
        if !profiles_list.contains(&profile_name.as_str()) {
            profiles_list.push(&profile_name);
        }
    }

    for profile_name in profiles_list {
        let profile = match node.profiles.get(profile_name) {
            Some(x) => x,
            None => good_panic!("No profile was found named `{}`", profile_name),
        };

        let mut merged_settings = top_settings.clone();
        merged_settings.merge(node.generic_settings.clone());
        merged_settings.merge(profile.generic_settings.clone());

        let deploy_data =
            utils::make_deploy_data(profile_name, node_name, &merged_settings).await?;

        utils::deploy::deploy_profile(
            profile,
            profile_name,
            node,
            node_name,
            &merged_settings,
            &deploy_data,
            merged_settings.auto_rollback,
        )
        .await?;
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

/// Evaluates the Nix in the given `repo` and return the processed Data from it
#[inline]
async fn get_deployment_data(
    supports_flakes: bool,
    repo: &str,
    extra_build_args: &[String],
) -> Result<utils::data::Data, Box<dyn std::error::Error>> {
    let mut c = match supports_flakes {
        true => Command::new("nix"),
        false => Command::new("nix-instanciate"),
    };

    let mut build_command = match supports_flakes {
        true => {
            c.arg("eval").arg("--json").arg(format!("{}#deploy", repo))
        }
        false => {
            c
                .arg("--strict")
                .arg("--read-write-mode")
                .arg("--json")
                .arg("--eval")
                .arg("--E")
                .arg(format!("let r = import {}/.; in if builtins.isFunction r then (r {{}}).deploy else r.deploy", repo))
        }
    };

    for extra_arg in extra_build_args {
        build_command = build_command.arg(extra_arg);
    }

    let build_output = build_command
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .await?;

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
    opts: &Opts,
) -> Result<(), Box<dyn std::error::Error>> {
    match (deploy_flake.node, deploy_flake.profile) {
        (Some(node_name), Some(profile_name)) => {
            let node = match data.nodes.get(node_name) {
                Some(x) => x,
                None => good_panic!("No node was found named `{}`", node_name),
            };
            let profile = match node.profiles.get(profile_name) {
                Some(x) => x,
                None => good_panic!("No profile was found named `{}`", profile_name),
            };

            let mut merged_settings = data.generic_settings.clone();
            merged_settings.merge(node.generic_settings.clone());
            merged_settings.merge(profile.generic_settings.clone());

            let deploy_data =
                utils::make_deploy_data(profile_name, node_name, &merged_settings).await?;

            utils::push::push_profile(
                profile,
                profile_name,
                node,
                node_name,
                supports_flakes,
                opts.checksigs,
                deploy_flake.repo,
                &merged_settings,
                &deploy_data,
            )
            .await?;

            utils::deploy::deploy_profile(
                profile,
                profile_name,
                node,
                node_name,
                &merged_settings,
                &deploy_data,
                merged_settings.auto_rollback,
            )
            .await?;
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
                opts.checksigs,
            )
            .await?;

            deploy_all_profiles(node, node_name, &data.generic_settings).await?;
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
                    opts.checksigs,
                )
                .await?;
            }

            for (node_name, node) in &data.nodes {
                deploy_all_profiles(node, node_name, &data.generic_settings).await?;
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

    let supports_flakes = test_flake_support().await?;

    let data =
        get_deployment_data(supports_flakes, deploy_flake.repo, &opts.extra_build_args).await?;

    run_deploy(deploy_flake, data, supports_flakes, &opts).await?;

    Ok(())
}
