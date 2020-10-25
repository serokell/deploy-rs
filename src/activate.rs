// SPDX-FileCopyrightText: 2020 Serokell <https://serokell.io/>
//
// SPDX-License-Identifier: MPL-2.0

use clap::Clap;

use futures_util::FutureExt;
use std::process::Stdio;
use tokio::fs;
use tokio::process::Command;
use tokio::time::timeout;

use std::time::Duration;

use futures_util::StreamExt;

use std::path::Path;

use inotify::Inotify;

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

    /// Temp path for any temporary files that may be needed during activation
    #[clap(long)]
    temp_path: String,

    /// Maximum time to wait for confirmation after activation
    #[clap(long)]
    confirm_timeout: u16,

    /// Wait for confirmation after deployment and rollback if not confirmed
    #[clap(long)]
    magic_rollback: bool,

    /// Command for bootstrapping
    #[clap(long)]
    bootstrap_cmd: Option<String>,

    /// Auto rollback if failure
    #[clap(long)]
    auto_rollback: bool,
}

pub async fn deactivate(profile_path: &str) -> Result<(), Box<dyn std::error::Error>> {
    error!("De-activating due to error");

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

    info!("Attempting to re-activate the last generation");

    let re_activate_exit_status = Command::new(format!("{}/deploy-rs-activate", profile_path))
        .env("PROFILE", &profile_path)
        .current_dir(&profile_path)
        .status()
        .await?;

    if !re_activate_exit_status.success() {
        good_panic!("Failed to re-activate the last generation");
    }

    Ok(())
}

async fn deactivate_on_err<A, B: core::fmt::Debug>(profile_path: &str, r: Result<A, B>) -> A {
    match r {
        Ok(x) => x,
        Err(err) => {
            error!("Deactivating due to error: {:?}", err);
            match deactivate(profile_path).await {
                Ok(_) => (),
                Err(err) => {
                    error!("Error de-activating, uh-oh: {:?}", err);
                }
            };

            std::process::exit(1);
        }
    }
}

pub async fn activation_confirmation(
    profile_path: String,
    temp_path: String,
    confirm_timeout: u16,
    closure: String,
) -> Result<(), Box<dyn std::error::Error>> {
    let lock_hash = &closure[11 /* /nix/store/ */ ..];
    let lock_path = format!("{}/activating-{}", temp_path, lock_hash);

    if let Some(parent) = Path::new(&lock_path).parent() {
        fs::create_dir_all(parent).await?;
    }

    fs::File::create(&lock_path).await?;

    let mut inotify = Inotify::init()?;
    inotify.add_watch(lock_path, inotify::WatchMask::DELETE)?;

    match fork::daemon(false, false).map_err(|x| x.to_string())? {
        fork::Fork::Child => {
            std::thread::spawn(move || {
                let mut rt = tokio::runtime::Runtime::new().unwrap();

                rt.block_on(async move {
                    info!("Waiting for confirmation event...");

                    let mut buffer = [0; 32];
                    let mut stream =
                        deactivate_on_err(&profile_path, inotify.event_stream(&mut buffer)).await;

                    deactivate_on_err(
                        &profile_path,
                        deactivate_on_err(
                            &profile_path,
                            deactivate_on_err(
                                &profile_path,
                                timeout(Duration::from_secs(confirm_timeout as u64), stream.next())
                                    .await,
                            )
                            .await
                            .ok_or("Watcher ended prematurely"),
                        )
                        .await,
                    )
                    .await;
                });
            })
            .join()
            .unwrap();

            info!("Confirmation successful!");

            std::process::exit(0);
        }
        fork::Fork::Parent(_) => {
            std::process::exit(0);
        }
    }
}

pub async fn activate(
    profile_path: String,
    closure: String,
    bootstrap_cmd: Option<String>,
    auto_rollback: bool,
    temp_path: String,
    confirm_timeout: u16,
    magic_rollback: bool,
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
        .current_dir(&profile_path)
        .status()
        .await;

    let activate_status_all = match activate_status {
        Ok(s) if s.success() => Ok(()),
        Ok(_) => Err(std::io::Error::new(std::io::ErrorKind::Other, "Activation did not succeed")),
        Err(x) => Err(x),
    };

    deactivate_on_err(&profile_path, activate_status_all).await;

    info!("Activation succeeded!");

    if magic_rollback {
        info!("Performing activation confirmation steps");
        deactivate_on_err(
            &profile_path,
            activation_confirmation(profile_path.clone(), temp_path, confirm_timeout, closure)
                .await,
        )
        .await;
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
        opts.temp_path,
        opts.confirm_timeout,
        opts.magic_rollback,
    )
    .await?;

    Ok(())
}
