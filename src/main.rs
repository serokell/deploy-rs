use clap::Clap;

use std::process::Stdio;
use tokio::process::Command;

use merge::Merge;

use std::path::Path;

extern crate pretty_env_logger;
#[macro_use]
extern crate log;

#[macro_use]
extern crate serde_derive;

#[macro_use]
mod utils;

/// Simple Rust rewrite of a simple Nix Flake deployment tool
#[derive(Clap, Debug)]
#[clap(version = "1.0", author = "notgne2 <gen2@gen2.space>")]
struct Opts {
    /// Log verbosity
    #[clap(short, long, parse(from_occurrences))]
    verbose: i32,

    #[clap(subcommand)]
    subcmd: SubCommand,
}

/// Deploy profiles
#[derive(Clap, Debug)]
struct DeployOpts {
    /// The flake to deploy
    #[clap(default_value = ".")]
    flake: String,
    /// Check signatures when using `nix copy`
    #[clap(short, long)]
    checksigs: bool,
}

/// Activate a profile on your current machine
#[derive(Clap, Debug)]
struct ActivateOpts {
    profile_path: String,
    closure: String,

    /// Command for activating the given profile
    #[clap(short, long)]
    activate_cmd: Option<String>,

    /// Command for bootstrapping
    #[clap(short, long)]
    bootstrap_cmd: Option<String>,

    /// Auto rollback if failure
    #[clap(short, long)]
    auto_rollback: bool,
}

#[derive(Clap, Debug)]
enum SubCommand {
    Deploy(DeployOpts),
    Activate(ActivateOpts),
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
        )
        .await?;
    }

    Ok(())
}

#[derive(PartialEq, Debug)]
struct DeployFlake<'a> {
    repo: &'a str,
    node: Option<&'a str>,
    profile: Option<&'a str>,
}

fn parse_flake(flake: &str) -> DeployFlake {
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

#[tokio::main]

async fn main() -> Result<(), Box<dyn std::error::Error>> {
    if std::env::var("DEPLOY_LOG").is_err() {
        std::env::set_var("DEPLOY_LOG", "info");
    }

    pretty_env_logger::init_custom_env("DEPLOY_LOG");

    let opts: Opts = Opts::parse();

    match opts.subcmd {
        SubCommand::Deploy(deploy_opts) => {
            let deploy_flake = parse_flake(deploy_opts.flake.as_str());

            let test_flake_status = Command::new("nix")
                .arg("eval")
                .arg("--expr")
                .arg("builtins.getFlake")
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .await?;

            let supports_flakes = test_flake_status.success();

            let data_json = match supports_flakes {
                true => {
                    let c = Command::new("nix")
                        .arg("eval")
                        .arg("--json")
                        .arg(format!("{}#deploy", deploy_flake.repo))
                        .stdout(Stdio::null())
                        .stderr(Stdio::null())
                        // TODO forward input args?
                        .output()
                        .await?;

                    String::from_utf8(c.stdout)?
                }
                false => {
                    let c = Command::new("nix-instanciate")
                        .arg("--strict")
                        .arg("--read-write-mode")
                        .arg("--json")
                        .arg("--eval")
                        .arg("--E")
                        .arg(format!("let r = import {}/.; in if builtins.isFunction r then (r {{}}).deploy else r.deploy", deploy_flake.repo))
                        .stdout(Stdio::null())
                        .stderr(Stdio::null())
                        .output()
                        .await?;

                    String::from_utf8(c.stdout)?
                }
            };

            let data: utils::data::Data = serde_json::from_str(&data_json)?;

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
                        deploy_opts.checksigs,
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
                        deploy_opts.checksigs,
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
                            deploy_opts.checksigs,
                        )
                        .await?;
                    }

                    for (node_name, node) in &data.nodes {
                        deploy_all_profiles(node, node_name, &data.generic_settings).await?;
                    }
                }
                (None, Some(_)) => good_panic!(
                    "Profile provided without a node, this is not (currently) supported"
                ),
            };
        }
        SubCommand::Activate(activate_opts) => {
            info!("Activating profile");

            Command::new("nix-env")
                .arg("-p")
                .arg(&activate_opts.profile_path)
                .arg("--set")
                .arg(&activate_opts.closure)
                .stdout(Stdio::null())
                .spawn()?
                .await?;

            if let (Some(bootstrap_cmd), false) = (
                activate_opts.bootstrap_cmd,
                !Path::new(&activate_opts.profile_path).exists(),
            ) {
                let bootstrap_status = Command::new("bash")
                    .arg("-c")
                    .arg(&bootstrap_cmd)
                    .env("PROFILE", &activate_opts.profile_path)
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .status()
                    .await;

                match bootstrap_status {
                    Ok(s) if s.success() => (),
                    _ => {
                        tokio::fs::remove_file(&activate_opts.profile_path).await?;
                        good_panic!("Failed to execute bootstrap command");
                    }
                }
            }

            if let Some(activate_cmd) = activate_opts.activate_cmd {
                let activate_status = Command::new("bash")
                    .arg("-c")
                    .arg(&activate_cmd)
                    .env("PROFILE", &activate_opts.profile_path)
                    .status()
                    .await;

                match activate_status {
                    Ok(s) if s.success() => (),
                    _ if activate_opts.auto_rollback => {
                        Command::new("nix-env")
                            .arg("-p")
                            .arg(&activate_opts.profile_path)
                            .arg("--rollback")
                            .stdout(Stdio::null())
                            .stderr(Stdio::null())
                            .spawn()?
                            .await?;

                        let c = Command::new("nix-env")
                            .arg("-p")
                            .arg(&activate_opts.profile_path)
                            .arg("--list-generations")
                            .output()
                            .await?;
                        let generations_list = String::from_utf8(c.stdout)?;

                        let last_generation_line = generations_list
                            .lines()
                            .last()
                            .expect("Expected to find a generation in list");

                        let last_generation_id = last_generation_line
                            .split_whitespace()
                            .next()
                            .expect("Expected to get ID from generation entry");

                        debug!("Removing generation entry {}", last_generation_line);
                        warn!("Removing generation by ID {}", last_generation_id);

                        Command::new("nix-env")
                            .arg("-p")
                            .arg(&activate_opts.profile_path)
                            .arg("--delete-generations")
                            .arg(last_generation_id)
                            .stdout(Stdio::null())
                            .stderr(Stdio::null())
                            .spawn()?
                            .await?;

                        // TODO: Find some way to make sure this command never changes, otherwise this will not work
                        Command::new("bash")
                            .arg("-c")
                            .arg(&activate_cmd)
                            .spawn()?
                            .await?;

                        good_panic!("Failed to execute activation command");
                    }
                    _ => {}
                }
            }
        }
    }

    Ok(())
}
