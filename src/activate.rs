// SPDX-FileCopyrightText: 2020 Serokell <https://serokell.io/>
//
// SPDX-License-Identifier: MPL-2.0

use clap::Clap;

use std::process::Stdio;
use tokio::process::Command;

use std::path::Path;

extern crate pretty_env_logger;
#[macro_use]
extern crate log;

#[macro_use]
extern crate serde_derive;

#[macro_use]
mod utils;

/// Activation portion of the simple Rust Nix deploy tool
#[derive(Clap, Debug)]
#[clap(version = "1.0", author = "notgne2 <gen2@gen2.space>")]
struct Opts {
    profile_path: String,
    closure: String,

    /// Command for bootstrapping
    #[clap(long)]
    bootstrap_cmd: Option<String>,

    /// Auto rollback if failure
    #[clap(long)]
    auto_rollback: bool,
}

pub async fn activate(
    profile_path: String,
    closure: String,
    bootstrap_cmd: Option<String>,
    auto_rollback: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    info!("Activating profile");

    let nix_env_set_exit_status = Command::new("nix-env")
        .arg("-p")
        .arg(&profile_path)
        .arg("--set")
        .arg(&closure)
        .stdout(Stdio::null())
        .status()
        .await?;

    if !nix_env_set_exit_status.success() {
        good_panic!("Failed to update nix-env generation");
    }

    if let (Some(bootstrap_cmd), false) = (bootstrap_cmd, !Path::new(&profile_path).exists()) {
        let bootstrap_status = Command::new("bash")
            .arg("-c")
            .arg(&bootstrap_cmd)
            .env("PROFILE", &profile_path)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .await;

        match bootstrap_status {
            Ok(s) if s.success() => (),
            _ => {
                tokio::fs::remove_file(&profile_path).await?;
                good_panic!("Failed to execute bootstrap command");
            }
        }
    }

    let activate_status = Command::new(format!("{}/deploy-rs-activate", profile_path))
        .env("PROFILE", &profile_path)
        .status()
        .await;

    match activate_status {
        Ok(s) if s.success() => (),
        _ if auto_rollback => {
            error!("Failed to execute activation command");

            let nix_env_rollback_exit_status = Command::new("nix-env")
                .arg("-p")
                .arg(&profile_path)
                .arg("--rollback")
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .await?;

            if !nix_env_rollback_exit_status.success() {
                good_panic!("`nix-env --rollback` failed");
            }

            debug!("Listing generations");

            let nix_env_list_generations_out = Command::new("nix-env")
                .arg("-p")
                .arg(&profile_path)
                .arg("--list-generations")
                .output()
                .await?;

            if !nix_env_list_generations_out.status.success() {
                good_panic!("Listing `nix-env` generations failed");
            }

            let generations_list = String::from_utf8(nix_env_list_generations_out.stdout)?;

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

            let nix_env_delete_generation_exit_status = Command::new("nix-env")
                .arg("-p")
                .arg(&profile_path)
                .arg("--delete-generations")
                .arg(last_generation_id)
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .await?;

            if !nix_env_delete_generation_exit_status.success() {
                good_panic!("Failed to delete failed generation");
            }

            info!("Attempting re-activate last generation");

            let re_activate_exit_status = Command::new(format!("{}/deploy-rs-activate", profile_path))
                .env("PROFILE", &profile_path)
                .status()
                .await?;

            if !re_activate_exit_status.success() {
                good_panic!("Failed to re-activate the last generation");
            }

            std::process::exit(1);
        }
        _ => {}
    }

    Ok(())
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    if std::env::var("DEPLOY_LOG").is_err() {
        std::env::set_var("DEPLOY_LOG", "info");
    }

    pretty_env_logger::init_custom_env("DEPLOY_LOG");

    let opts: Opts = Opts::parse();

    activate(
        opts.profile_path,
        opts.closure,
        opts.bootstrap_cmd,
        opts.auto_rollback,
    )
    .await?;

    Ok(())
}
