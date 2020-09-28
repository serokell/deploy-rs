use clap::Clap;
use merge::Merge;
use std::{collections::HashMap, path::PathBuf};

use std::borrow::Cow;

use std::process::Stdio;
use tokio::process::Command;

use std::path::Path;

use std::process;

extern crate pretty_env_logger;
#[macro_use]
extern crate log;

#[macro_use]
extern crate serde_derive;

macro_rules! good_panic {
    ($($tts:tt)*) => {{
        error!($($tts)*);
        process::exit(1);
    }}
}

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
    /// Prepare server (for first deployments)
    #[clap(short, long)]
    prime: bool,
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

#[derive(Deserialize, Debug, Clone, Merge)]
pub struct GenericSettings {
    #[serde(rename(deserialize = "sshUser"))]
    pub ssh_user: Option<String>,
    pub user: Option<String>,
    #[serde(
        skip_serializing_if = "Vec::is_empty",
        default,
        rename(deserialize = "sshOpts")
    )]
    #[merge(strategy = merge::vec::append)]
    pub ssh_opts: Vec<String>,
    #[serde(rename(deserialize = "fastConnection"))]
    pub fast_connection: Option<bool>,
    #[serde(rename(deserialize = "autoRollback"))]
    pub auto_rollback: Option<String>,
}

#[derive(Deserialize, Debug, Clone)]
pub struct NodeSettings {
    pub hostname: String,
}

#[derive(Deserialize, Debug, Clone)]
pub struct ProfileSettings {
    pub path: String,
    pub activate: Option<String>,
    pub bootstrap: Option<String>,
}

#[derive(Deserialize, Debug, Clone)]
pub struct Profile {
    #[serde(flatten)]
    pub profile_settings: ProfileSettings,
    #[serde(flatten)]
    pub generic_settings: GenericSettings,
}

#[derive(Deserialize, Debug, Clone)]
pub struct Node {
    #[serde(flatten)]
    pub generic_settings: GenericSettings,
    #[serde(flatten)]
    pub node_settings: NodeSettings,

    pub profiles: HashMap<String, Profile>,
    #[serde(
        skip_serializing_if = "Vec::is_empty",
        default,
        rename(deserialize = "profilesOrder")
    )]
    pub profiles_order: Vec<String>,
}

#[derive(Deserialize, Debug, Clone)]
pub struct Data {
    #[serde(flatten)]
    pub generic_settings: GenericSettings,
    pub nodes: HashMap<String, Node>,
}

struct DeployData<'a> {
    pub sudo: Option<String>,
    pub ssh_user: Cow<'a, str>,
    pub profile_user: Cow<'a, str>,
    pub profile_path: String,
    pub current_exe: PathBuf,
}

async fn make_deploy_data<'a>(
    profile_name: &str,
    node_name: &str,
    merged_settings: &'a GenericSettings,
) -> Result<DeployData<'a>, Box<dyn std::error::Error>> {
    let ssh_user: Cow<str> = match &merged_settings.ssh_user {
        Some(u) => u.into(),
        None => whoami::username().into(),
    };

    let profile_user: Cow<str> = match &merged_settings.user {
        Some(x) => x.into(),
        None => match &merged_settings.ssh_user {
            Some(x) => x.into(),
            None => good_panic!(
                "Neither user nor sshUser set for profile `{}` of node `{}`",
                profile_name,
                node_name
            ),
        },
    };

    let profile_path = match &profile_user[..] {
        "root" => format!("/nix/var/nix/profiles/{}", profile_name),
        _ => format!(
            "/nix/var/nix/profiles/per-user/{}/{}",
            profile_user, profile_name
        ),
    };

    let sudo: Option<String> = match merged_settings.user {
        Some(ref user) if user != &ssh_user => Some(format!("sudo -u {}", user).into()),
        _ => None,
    };

    let current_exe = std::env::current_exe().expect("Expected to find current executable path");

    if !current_exe.starts_with("/nix/store/") {
        good_panic!("The deploy binary must be in the Nix store");
    }

    Ok(DeployData {
        sudo,
        ssh_user,
        profile_user,
        profile_path,
        current_exe,
    })
}

async fn push_profile(
    profile: &Profile,
    profile_name: &str,
    node: &Node,
    node_name: &str,
    supports_flakes: bool,
    check_sigs: bool,
    repo: &str,
    merged_settings: &GenericSettings,
    deploy_data: &DeployData<'_>,
) -> Result<(), Box<dyn std::error::Error>> {
    info!(
        "Deploying profile `{}` for node `{}`",
        profile_name, node_name
    );

    info!(
        "Building profile `{}` for node `{}`",
        profile_name, node_name
    );
    if supports_flakes {
        Command::new("nix")
            .arg("build")
            .arg("--no-link")
            .arg(format!(
                "{}#deploy.nodes.{}.profiles.{}.path",
                repo, node_name, profile_name
            ))
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()?
            .await?;
    } else {
        Command::new("nix-build")
            .arg(&repo)
            .arg("-A")
            .arg(format!(
                "deploy.nodes.{}.profiles.{}.path",
                node_name, profile_name
            ))
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()?
            .await?;
    }

    if let Ok(local_key) = std::env::var("LOCAL_KEY") {
        info!(
            "Signing key present! Signing profile `{}` for node `{}`",
            profile_name, node_name
        );

        Command::new("nix")
            .arg("sign-paths")
            .arg("-r")
            .arg("-k")
            .arg(local_key)
            .arg(&profile.profile_settings.path)
            .arg(&deploy_data.current_exe)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()?
            .await?;
    }

    info!("Copying profile `{} for node `{}`", profile_name, node_name);

    let mut copy_command_ = Command::new("nix");
    let mut copy_command = copy_command_.arg("copy");

    if let Some(true) = merged_settings.fast_connection {
        copy_command = copy_command.arg("--substitute-on-destination");
    }

    if !check_sigs {
        copy_command = copy_command.arg("--no-check-sigs");
    }

    let ssh_opts_str = merged_settings
        .ssh_opts
        // This should provide some extra safety, but it also breaks for some reason, oh well
        // .iter()
        // .map(|x| format!("'{}'", x))
        // .collect::<Vec<String>>()
        .join(" ");

    copy_command
        .arg("--to")
        .arg(format!(
            "ssh://{}@{}",
            deploy_data.ssh_user, node.node_settings.hostname
        ))
        .arg(&profile.profile_settings.path)
        .arg(&deploy_data.current_exe)
        .env("NIX_SSHOPTS", ssh_opts_str)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?
        .await?;

    Ok(())
}

async fn deploy_profile(
    profile: &Profile,
    profile_name: &str,
    node: &Node,
    node_name: &str,
    merged_settings: &GenericSettings,
    deploy_data: &DeployData<'_>,
) -> Result<(), Box<dyn std::error::Error>> {
    info!(
        "Activating profile `{}` for node `{}`",
        profile_name, node_name
    );

    let mut self_activate_command = format!(
        "{} activate '{}' '{}'",
        deploy_data.current_exe.as_path().to_str().unwrap(),
        deploy_data.profile_path,
        profile.profile_settings.path,
    );

    if let Some(sudo_cmd) = &deploy_data.sudo {
        self_activate_command = format!("{} {}", sudo_cmd, self_activate_command);
    }

    if let Some(ref bootstrap_cmd) = profile.profile_settings.bootstrap {
        self_activate_command = format!(
            "{} --bootstrap-cmd '{}'",
            self_activate_command, bootstrap_cmd
        );
    }

    if let Some(ref activate_cmd) = profile.profile_settings.activate {
        self_activate_command = format!(
            "{} --activate-cmd '{}'",
            self_activate_command, activate_cmd
        );
    }

    let mut c = Command::new("ssh");
    let mut ssh_command = c.arg(format!(
        "ssh://{}@{}",
        deploy_data.ssh_user, node.node_settings.hostname
    ));

    for ssh_opt in &merged_settings.ssh_opts {
        ssh_command = ssh_command.arg(ssh_opt);
    }

    ssh_command.arg(self_activate_command).spawn()?.await?;

    Ok(())
}

#[inline]
async fn push_all_profiles(
    node: &Node,
    node_name: &str,
    supports_flakes: bool,
    repo: &str,
    top_settings: &GenericSettings,
    check_sigs: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    info!("Deploying all profiles for `{}`", node_name);

    let mut profiles_list: Vec<&str> = node.profiles_order.iter().map(|x| x.as_ref()).collect();

    // Add any profiles which weren't in the provided order list
    for (profile_name, _) in &node.profiles {
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

        let deploy_data = make_deploy_data(profile_name, node_name, &merged_settings).await?;

        push_profile(
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
    node: &Node,
    node_name: &str,
    top_settings: &GenericSettings,
) -> Result<(), Box<dyn std::error::Error>> {
    info!("Deploying all profiles for `{}`", node_name);

    let mut profiles_list: Vec<&str> = node.profiles_order.iter().map(|x| x.as_ref()).collect();

    // Add any profiles which weren't in the provided order list
    for (profile_name, _) in &node.profiles {
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

        let deploy_data = make_deploy_data(profile_name, node_name, &merged_settings).await?;

        deploy_profile(
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

#[tokio::main]

async fn main() -> Result<(), Box<dyn std::error::Error>> {
    if let Err(_) = std::env::var("DEPLOY_LOG") {
        std::env::set_var("DEPLOY_LOG", "info");
    }

    pretty_env_logger::init_custom_env("DEPLOY_LOG");

    let opts: Opts = Opts::parse();

    match opts.subcmd {
        SubCommand::Deploy(deploy_opts) => {
            let flake_fragment_start = deploy_opts.flake.find('#');
            let (repo, maybe_fragment) = match flake_fragment_start {
                Some(s) => (&deploy_opts.flake[..s], Some(&deploy_opts.flake[s + 1..])),
                None => (deploy_opts.flake.as_str(), None),
            };

            let (maybe_node, maybe_profile) = match maybe_fragment {
                Some(fragment) => {
                    let fragment_profile_start = fragment.find('.');
                    match fragment_profile_start {
                        Some(s) => (Some(&fragment[..s]), Some(&fragment[s + 1..])),
                        None => (Some(fragment), None),
                    }
                }
                None => (None, None),
            };

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
                        .arg(format!("{}#deploy", repo))
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
                        .arg(format!("let r = import {}/.; in if builtins.isFunction r then (r {{}}).deploy else r.deploy", repo))
                        .stdout(Stdio::null())
                        .stderr(Stdio::null())
                        .output()
                        .await?;

                    String::from_utf8(c.stdout)?
                }
            };

            let data: Data = serde_json::from_str(&data_json)?;

            match (maybe_node, maybe_profile) {
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
                        make_deploy_data(profile_name, node_name, &merged_settings).await?;

                    push_profile(
                        profile,
                        profile_name,
                        node,
                        node_name,
                        supports_flakes,
                        deploy_opts.checksigs,
                        repo,
                        &merged_settings,
                        &deploy_data,
                    )
                    .await?;

                    deploy_profile(
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
                        repo,
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
                            repo,
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

                        // TODO: why are we doing this?
                        // to run the older version as long as the command is the same?
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
