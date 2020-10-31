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

use thiserror::Error;

extern crate pretty_env_logger;
#[macro_use]
extern crate log;

#[macro_use]
extern crate serde_derive;

#[macro_use]
mod utils;

/// Activation portion of the simple Rust Nix deploy tool
#[derive(Clap, Debug)]
#[clap(version = "1.0", author = "Serokell <https://serokell.io/>")]
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

#[derive(Error, Debug)]
pub enum DeactivateError {
    #[error("Failed to execute the rollback command: {0}")]
    RollbackError(std::io::Error),
    #[error("The rollback resulted in a bad exit code: {0:?}")]
    RollbackExitError(Option<i32>),
    #[error("Failed to run command for listing generations: {0}")]
    ListGenError(std::io::Error),
    #[error("Command for listing generations resulted in a bad exit code: {0:?}")]
    ListGenExitError(Option<i32>),
    #[error("Error converting generation list output to utf8: {0}")]
    DecodeListGenUtf8Error(#[from] std::string::FromUtf8Error),
    #[error("Failed to run command for deleting generation: {0}")]
    DeleteGenError(std::io::Error),
    #[error("Command for deleting generations resulted in a bad exit code: {0:?}")]
    DeleteGenExitError(Option<i32>),
    #[error("Failed to run command for re-activating the last generation: {0}")]
    ReactivateError(std::io::Error),
    #[error("Command for re-activating the last generation resulted in a bad exit code: {0:?}")]
    ReactivateExitError(Option<i32>),
}

pub async fn deactivate(profile_path: &str) -> Result<(), DeactivateError> {
    error!("De-activating due to error");

    let nix_env_rollback_exit_status = Command::new("nix-env")
        .arg("-p")
        .arg(&profile_path)
        .arg("--rollback")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .await
        .map_err(DeactivateError::RollbackError)?;

    match nix_env_rollback_exit_status.code() {
        Some(0) => (),
        a => return Err(DeactivateError::RollbackExitError(a)),
    };

    debug!("Listing generations");

    let nix_env_list_generations_out = Command::new("nix-env")
        .arg("-p")
        .arg(&profile_path)
        .arg("--list-generations")
        .output()
        .await
        .map_err(DeactivateError::ListGenError)?;

    match nix_env_list_generations_out.status.code() {
        Some(0) => (),
        a => return Err(DeactivateError::ListGenExitError(a)),
    };

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
        .await
        .map_err(DeactivateError::DeleteGenError)?;

    match nix_env_delete_generation_exit_status.code() {
        Some(0) => (),
        a => return Err(DeactivateError::DeleteGenExitError(a)),
    };

    info!("Attempting to re-activate the last generation");

    let re_activate_exit_status = Command::new(format!("{}/deploy-rs-activate", profile_path))
        .env("PROFILE", &profile_path)
        .current_dir(&profile_path)
        .status()
        .await
        .map_err(DeactivateError::ReactivateError)?;

    match re_activate_exit_status.code() {
        Some(0) => (),
        a => return Err(DeactivateError::ReactivateExitError(a)),
    };

    Ok(())
}

#[derive(Error, Debug)]
pub enum ActivationConfirmationError {
    #[error("Failed to create activation confirmation directory: {0}")]
    CreateConfirmDirError(std::io::Error),
    #[error("Failed to create activation confirmation file: {0}")]
    CreateConfirmFileError(std::io::Error),
    #[error("Failed to create inotify instance: {0}")]
    CreateInotifyError(std::io::Error),
    #[error("Failed to create inotify watcher: {0}")]
    CreateInotifyWatcherError(std::io::Error),
    #[error("Error forking process: {0}")]
    ForkError(i32),
}

#[derive(Error, Debug)]
pub enum DangerZoneError {
    #[error("Timeout elapsed for confirmation: {0}")]
    TimesUp(#[from] tokio::time::Elapsed),
    #[error("inotify stream ended without activation confirmation")]
    NoConfirmation,
    #[error("There was some kind of error waiting for confirmation (todo figure it out)")]
    SomeKindOfError(std::io::Error),
}

async fn danger_zone(
    profile_path: &str,
    mut inotify: Inotify,
    confirm_timeout: u16,
) -> Result<(), DangerZoneError> {
    info!("Waiting for confirmation event...");

    let mut buffer = [0; 32];
    let mut stream = inotify
        .event_stream(&mut buffer)
        .map_err(DangerZoneError::SomeKindOfError)?;

    timeout(Duration::from_secs(confirm_timeout as u64), stream.next())
        .await?
        .ok_or(DangerZoneError::NoConfirmation)?
        .map_err(DangerZoneError::SomeKindOfError)?;

    Ok(())
}

pub async fn activation_confirmation(
    profile_path: String,
    temp_path: String,
    confirm_timeout: u16,
    closure: String,
) -> Result<(), ActivationConfirmationError> {
    let lock_hash = &closure["/nix/store/".len()..];
    let lock_path = format!("{}/activating-{}", temp_path, lock_hash);

    if let Some(parent) = Path::new(&lock_path).parent() {
        fs::create_dir_all(parent)
            .await
            .map_err(ActivationConfirmationError::CreateConfirmDirError)?;
    }

    fs::File::create(&lock_path)
        .await
        .map_err(ActivationConfirmationError::CreateConfirmDirError)?;

    let mut inotify =
        Inotify::init().map_err(ActivationConfirmationError::CreateConfirmDirError)?;
    inotify
        .add_watch(lock_path, inotify::WatchMask::DELETE)
        .map_err(ActivationConfirmationError::CreateConfirmDirError)?;

    if let fork::Fork::Child =
        fork::daemon(false, false).map_err(ActivationConfirmationError::ForkError)?
    {
        std::thread::spawn(move || {
                let mut rt = tokio::runtime::Runtime::new().unwrap();

                rt.block_on(async move {
                    if let Err(err) = danger_zone(&profile_path, inotify, confirm_timeout).await {
                        if let Err(err) = deactivate(&profile_path).await {
                            good_panic!("Error de-activating due to another error in confirmation thread, oh no...: {}", err);
                        }

                        good_panic!("Error in confirmation thread: {}", err);
                    }
                });
            })
            .join()
            .unwrap();

        info!("Confirmation successful!");
    }

    std::process::exit(0);
}

#[derive(Error, Debug)]
pub enum ActivateError {
    #[error("Failed to execute the command for setting profile: {0}")]
    SetProfileError(std::io::Error),
    #[error("The command for setting profile resulted in a bad exit code: {0:?}")]
    SetProfileExitError(Option<i32>),

    #[error("Failed to run bootstrap command: {0}")]
    BootstrapError(std::io::Error),
    #[error("The bootstrap command resulted in a bad exit code: {0:?}")]
    BootstrapExitError(Option<i32>),

    #[error("Error removing profile after bootstrap failed: {0}")]
    RemoveGenerationErr(std::io::Error),

    #[error("Failed to execute the activation script: {0}")]
    RunActivateError(std::io::Error),
    #[error("The activation script resulted in a bad exit code: {0:?}")]
    RunActivateExitError(Option<i32>),

    #[error("There was an error de-activating after an error was encountered: {0}")]
    DeactivateError(#[from] DeactivateError),

    #[error("Failed to get activation confirmation: {0}")]
    ActivationConfirmationError(#[from] ActivationConfirmationError),
}

pub async fn activate(
    profile_path: String,
    closure: String,
    bootstrap_cmd: Option<String>,
    auto_rollback: bool,
    temp_path: String,
    confirm_timeout: u16,
    magic_rollback: bool,
) -> Result<(), ActivateError> {
    info!("Activating profile");

    let nix_env_set_exit_status = Command::new("nix-env")
        .arg("-p")
        .arg(&profile_path)
        .arg("--set")
        .arg(&closure)
        .stdout(Stdio::null())
        .status()
        .await
        .map_err(ActivateError::SetProfileError)?;

    match nix_env_set_exit_status.code() {
        Some(0) => (),
        a => {
            deactivate(&profile_path).await?;
            return Err(ActivateError::SetProfileExitError(a));
        }
    };

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
            Ok(s) => match s.code() {
                Some(0) => {}
                a => {
                    tokio::fs::remove_file(&profile_path)
                        .await
                        .map_err(ActivateError::RemoveGenerationErr)?;

                    return Err(ActivateError::BootstrapExitError(a));
                }
            },
            Err(err) => {
                tokio::fs::remove_file(&profile_path)
                    .await
                    .map_err(ActivateError::RemoveGenerationErr)?;

                return Err(ActivateError::BootstrapError(err));
            }
        }
    }

    let activate_status = match Command::new(format!("{}/deploy-rs-activate", profile_path))
        .env("PROFILE", &profile_path)
        .current_dir(&profile_path)
        .status()
        .await
        .map_err(ActivateError::RunActivateError)
    {
        Ok(x) => x,
        Err(e) => {
            deactivate(&profile_path).await?;
            return Err(e);
        }
    };

    match activate_status.code() {
        Some(0) => (),
        a => {
            deactivate(&profile_path).await?;
            return Err(ActivateError::RunActivateExitError(a));
        }
    };

    info!("Activation succeeded!");

    if magic_rollback {
        match activation_confirmation(profile_path.clone(), temp_path, confirm_timeout, closure)
            .await
        {
            Ok(()) => {}
            Err(err) => {
                deactivate(&profile_path).await?;
                return Err(ActivateError::ActivationConfirmationError(err));
            }
        };
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

    match activate(
        opts.profile_path,
        opts.closure,
        opts.bootstrap_cmd,
        opts.auto_rollback,
        opts.temp_path,
        opts.confirm_timeout,
        opts.magic_rollback,
    )
    .await
    {
        Ok(()) => (),
        Err(err) => good_panic!("{}", err),
    }

    Ok(())
}
