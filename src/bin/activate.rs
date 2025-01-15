// SPDX-FileCopyrightText: 2020 Serokell <https://serokell.io/>
// SPDX-FileCopyrightText: 2020 Andreas Fuchs <asf@boinkor.net>
// SPDX-FileCopyrightText: 2021 Yannik Sander <contact@ysndr.de>
//
// SPDX-License-Identifier: MPL-2.0

use signal_hook::{consts::signal::SIGHUP, iterator::Signals};

use clap::Clap;

use tokio::fs;
use tokio::process::Command;
use tokio::sync::mpsc;
use tokio::time::timeout;

use std::time::Duration;

use std::env;
use std::path::{Path, PathBuf};

use notify::{recommended_watcher, RecommendedWatcher, RecursiveMode, Watcher};

use thiserror::Error;

use log::{debug, error, info, warn};

use deploy::command;

/// Remote activation utility for deploy-rs
#[derive(Clap, Debug)]
#[clap(version = "1.0", author = "Serokell <https://serokell.io/>")]
struct Opts {
    /// Print debug logs to output
    #[clap(short, long)]
    debug_logs: bool,
    /// Directory to print logs to
    #[clap(long)]
    log_dir: Option<String>,

    #[clap(subcommand)]
    subcmd: SubCommand,
}

#[derive(Clap, Debug)]
enum SubCommand {
    Activate(ActivateOpts),
    Wait(WaitOpts),
    Revoke(RevokeOpts),
}

/// Activate a profile
#[derive(Clap, Debug)]
#[clap(group(
    clap::ArgGroup::new("profile")
        .required(true)
        .multiple(false)
        .args(&["profile-path","profile-user"])
))]
struct ActivateOpts {
    /// The closure to activate
    closure: String,
    /// The profile path to install into
    #[clap(long)]
    profile_path: Option<String>,
    /// The profile user if explicit profile path is not specified
    #[clap(long, requires = "profile-name")]
    profile_user: Option<String>,
    /// The profile name
    #[clap(long, requires = "profile-user")]
    profile_name: Option<String>,

    /// Maximum time to wait for confirmation after activation
    #[clap(long)]
    confirm_timeout: u16,

    /// Wait for confirmation after deployment and rollback if not confirmed
    #[clap(long)]
    magic_rollback: bool,

    /// Auto rollback if failure
    #[clap(long)]
    auto_rollback: bool,

    /// Show what will be activated on the machines
    #[clap(long)]
    dry_activate: bool,

    /// Don't activate, but update the boot loader to boot into the new profile
    #[clap(long)]
    boot: bool,

    /// Path for any temporary files that may be needed during activation
    #[clap(long)]
    temp_path: PathBuf,
}

/// Wait for profile activation
#[derive(Clap, Debug)]
struct WaitOpts {
    /// The closure to wait for
    closure: String,

    /// Path for any temporary files that may be needed during activation
    #[clap(long)]
    temp_path: PathBuf,

    /// Timeout to wait for activation
    #[clap(long)]
    activation_timeout: Option<u16>,
}

/// Revoke profile activation
#[derive(Clap, Debug)]
struct RevokeOpts {
    /// The profile path to install into
    #[clap(long)]
    profile_path: Option<String>,
    /// The profile user if explicit profile path is not specified
    #[clap(long, requires = "profile-name")]
    profile_user: Option<String>,
    /// The profile name
    #[clap(long, requires = "profile-user")]
    profile_name: Option<String>,
}

#[derive(Error, Debug)]
pub enum RollbackError {}

impl command::HasCommandError for RollbackError {
    fn title() -> String {
        "Nix rollback".to_string()
    }
}

#[derive(Error, Debug)]
pub enum ListGenError {}

impl command::HasCommandError for ListGenError {
    fn title() -> String {
        "Nix list generations".to_string()
    }
}

#[derive(Error, Debug)]
pub enum DeleteGenError {}

impl command::HasCommandError for DeleteGenError {
    fn title() -> String {
        "Nix delete generations".to_string()
    }
}

#[derive(Error, Debug)]
pub enum ReactivateError {}

impl command::HasCommandError for ReactivateError {
    fn title() -> String {
        "Nix reactivate last generation".to_string()
    }
}

#[derive(Error, Debug)]
pub enum DeactivateError {
    #[error("{0}")]
    Rollback(#[from] command::CommandError<RollbackError>),
    #[error("{0}")]
    ListGen(#[from] command::CommandError<ListGenError>),
    #[error("Error converting generation list output to utf8: {0}")]
    DecodeListGenUtf8(std::string::FromUtf8Error),
    #[error("{0}")]
    DeleteGen(#[from] command::CommandError<DeleteGenError>),
    #[error("{0}")]
    Reactivate(#[from] command::CommandError<ReactivateError>),
}

pub async fn deactivate(profile_path: &str) -> Result<(), DeactivateError> {
    warn!("De-activating due to error");

    let mut nix_env_rollback_command = Command::new("nix-env");
    nix_env_rollback_command
        .arg("-p")
        .arg(&profile_path)
        .arg("--rollback");
    command::Command::new(nix_env_rollback_command)
        .run()
        .await
        .map_err(DeactivateError::Rollback)?;

    debug!("Listing generations");

    let mut nix_env_list_generations_command = Command::new("nix-env");
    nix_env_list_generations_command
        .arg("-p")
        .arg(&profile_path)
        .arg("--list-generations");
    let nix_env_list_generations_out = command::Command::new(nix_env_list_generations_command)
        .run()
        .await
        .map_err(DeactivateError::ListGen)?;

    let generations_list = String::from_utf8(nix_env_list_generations_out.stdout)
        .map_err(DeactivateError::DecodeListGenUtf8)?;

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

    let mut nix_env_delete_generation_command = Command::new("nix-env");
    nix_env_delete_generation_command
        .arg("-p")
        .arg(&profile_path)
        .arg("--delete-generations")
        .arg(last_generation_id);
    command::Command::new(nix_env_delete_generation_command)
        .run()
        .await
        .map_err(DeactivateError::DeleteGen)?;

    info!("Attempting to re-activate the last generation");

    let mut re_activate_command = Command::new(format!("{}/deploy-rs-activate", profile_path));
    re_activate_command
        .env("PROFILE", &profile_path)
        .current_dir(&profile_path);
    command::Command::new(re_activate_command)
        .run()
        .await
        .map_err(DeactivateError::Reactivate)?;

    Ok(())
}

#[derive(Error, Debug)]
pub enum ActivationConfirmationError {
    #[error("Failed to create activation confirmation directory: {0}")]
    CreateConfirmDir(std::io::Error),
    #[error("Failed to create activation confirmation file: {0}")]
    CreateConfirmFile(std::io::Error),
    #[error("Could not watch for activation sentinel: {0}")]
    Watcher(#[from] notify::Error),
    #[error("Error waiting for confirmation event: {0}")]
    WaitingError(#[from] DangerZoneError),
}

#[derive(Error, Debug)]
pub enum DangerZoneError {
    #[error("Timeout elapsed for confirmation")]
    TimesUp,
    #[error("inotify stream ended without activation confirmation")]
    NoConfirmation,
    #[error("inotify encountered an error: {0}")]
    Watch(notify::Error),
}

async fn danger_zone(
    mut events: mpsc::Receiver<Result<(), notify::Error>>,
    confirm_timeout: u16,
) -> Result<(), DangerZoneError> {
    info!("Waiting for confirmation event...");

    match timeout(Duration::from_secs(confirm_timeout as u64), events.recv()).await {
        Ok(Some(Ok(()))) => Ok(()),
        Ok(Some(Err(e))) => Err(DangerZoneError::Watch(e)),
        Ok(None) => Err(DangerZoneError::NoConfirmation),
        Err(_) => Err(DangerZoneError::TimesUp),
    }
}

pub async fn activation_confirmation(
    temp_path: PathBuf,
    confirm_timeout: u16,
    closure: String,
) -> Result<(), ActivationConfirmationError> {
    let lock_path = deploy::make_lock_path(&temp_path, &closure);

    debug!("Ensuring parent directory exists for canary file");

    if let Some(parent) = lock_path.parent() {
        fs::create_dir_all(parent)
            .await
            .map_err(ActivationConfirmationError::CreateConfirmDir)?;
    }

    debug!("Creating canary file");

    fs::File::create(&lock_path)
        .await
        .map_err(ActivationConfirmationError::CreateConfirmFile)?;

    debug!("Creating notify watcher");

    let (deleted, done) = mpsc::channel(1);

    let mut watcher: RecommendedWatcher =
        recommended_watcher(move |res: Result<notify::event::Event, notify::Error>| {
            let send_result = match res {
                Ok(e) if e.kind == notify::EventKind::Remove(notify::event::RemoveKind::File) => {
                    debug!("Got worthy removal event, sending on channel");
                    deleted.try_send(Ok(()))
                }
                Err(e) => {
                    debug!("Got error waiting for removal event, sending on channel");
                    deleted.try_send(Err(e))
                }
                Ok(_) => Ok(()), // ignore non-removal events
            };

            if let Err(e) = send_result {
                error!("Could not send file system event to watcher: {}", e);
            }
        })?;

    watcher.watch(&lock_path, RecursiveMode::NonRecursive)?;

    danger_zone(done, confirm_timeout)
        .await
        .map_err(|err| ActivationConfirmationError::WaitingError(err))
}

#[derive(Error, Debug)]
pub enum WaitError {
    #[error("Error creating watcher for activation: {0}")]
    Watcher(#[from] notify::Error),
    #[error("Error waiting for activation: {0}")]
    Waiting(#[from] DangerZoneError),
}
pub async fn wait(temp_path: PathBuf, closure: String, activation_timeout: Option<u16>) -> Result<(), WaitError> {
    let lock_path = deploy::make_lock_path(&temp_path, &closure);

    let (created, done) = mpsc::channel(1);

    let mut watcher: RecommendedWatcher = {
        // TODO: fix wasteful clone
        let lock_path = lock_path.clone();

        recommended_watcher(move |res: Result<notify::event::Event, notify::Error>| {
            let send_result = match res {
                Ok(e) if e.kind == notify::EventKind::Create(notify::event::CreateKind::File) => {
                    match &e.paths[..] {
                        [x] => match lock_path.canonicalize() {
                            // 'lock_path' may not exist yet when some other files are created in 'temp_path'
                            // x is already supposed to be canonical path
                            Ok(lock_path) if x == &lock_path => created.try_send(Ok(())),
                            _ => Ok(()),
                        },
                        _ => Ok(()),
                    }
                }
                Err(e) => created.try_send(Err(e)),
                Ok(_) => Ok(()), // ignore non-removal events
            };

            if let Err(e) = send_result {
                error!("Could not send file system event to watcher: {}", e);
            }
        })?
    };

    watcher.watch(&temp_path, RecursiveMode::NonRecursive)?;

    // Avoid a potential race condition by checking for existence after watcher creation
    if fs::metadata(&lock_path).await.is_ok() {
        watcher.unwatch(&temp_path)?;
        return Ok(());
    }

    danger_zone(done, activation_timeout.unwrap_or(240)).await?;

    info!("Found canary file, done waiting!");

    Ok(())
}

#[derive(Error, Debug)]
pub enum SetProfileError {}

impl command::HasCommandError for SetProfileError {
    fn title() -> String {
        "Nix profile set".to_string()
    }
}

#[derive(Error, Debug)]
pub enum RunActivateError {}

impl command::HasCommandError for RunActivateError {
    fn title() -> String {
        "Nix activation script".to_string()
    }
}

#[derive(Error, Debug)]
pub enum ActivateError {
    #[error("{0}")]
    SetProfile(#[from] command::CommandError<SetProfileError>),

    #[error("{0}")]
    RunActivate(#[from] command::CommandError<RunActivateError>),

    #[error("There was an error de-activating after an error was encountered: {0}")]
    Deactivate(#[from] DeactivateError),

    #[error("Failed to get activation confirmation: {0}")]
    ActivationConfirmation(#[from] ActivationConfirmationError),
}

pub async fn activate(
    profile_path: String,
    closure: String,
    auto_rollback: bool,
    temp_path: PathBuf,
    confirm_timeout: u16,
    magic_rollback: bool,
    dry_activate: bool,
    boot: bool,
) -> Result<(), ActivateError> {
    if !dry_activate {
        info!("Activating profile");
        let mut nix_env_set_command = Command::new("nix-env");
        nix_env_set_command
            .arg("-p")
            .arg(&profile_path)
            .arg("--set")
            .arg(&closure);
        let nix_env_set_exit_output = nix_env_set_command
            .output()
            .await
            .map_err(|err| {
                ActivateError::SetProfile(command::CommandError::RunError(err))
            })?;
        match nix_env_set_exit_output.status.code() {
            Some(0) => (),
            _exit_code => {
                if auto_rollback && !dry_activate {
                    deactivate(&profile_path).await?;
                }
                return Err(ActivateError::SetProfile(
                    command::CommandError::Exit(nix_env_set_exit_output, format!("{:?}", nix_env_set_command))
                ));
            }
        };
    }

    debug!("Running activation script");

    let activation_location = if dry_activate {
        &closure
    } else {
        &profile_path
    };

    let mut activate_command = Command::new(format!("{}/deploy-rs-activate", activation_location));
    activate_command
        .env("PROFILE", activation_location)
        .env("DRY_ACTIVATE", if dry_activate { "1" } else { "0" })
        .env("BOOT", if boot { "1" } else { "0" })
        .current_dir(activation_location);
    let activate_output = match activate_command
        .output()
        .await
        .map_err(|err| {
            ActivateError::RunActivate(command::CommandError::RunError(err))
        })
    {
        Ok(x) => x,
        Err(e) => {
            if auto_rollback && !dry_activate {
                deactivate(&profile_path).await?;
            }
            return Err(e);
        }
    };

    if !dry_activate {
        match activate_output.status.code() {
            Some(0) => (),
            _exit_code => {
                if auto_rollback {
                    deactivate(&profile_path).await?;
                }
                return Err(ActivateError::RunActivate(
                    command::CommandError::Exit(activate_output, format!("{:?}", activate_command))
                ));
            }
        };

        if !dry_activate {
            info!("Activation succeeded!");
        }

        if magic_rollback && !boot {
            info!("Magic rollback is enabled, setting up confirmation hook...");
            if let Err(err) = activation_confirmation(temp_path, confirm_timeout, closure).await {
                deactivate(&profile_path).await?;
                return Err(ActivateError::ActivationConfirmation(err));
            }
        }
    }

    Ok(())
}

async fn revoke(profile_path: String) -> Result<(), DeactivateError> {
    deactivate(profile_path.as_str()).await?;
    Ok(())
}

#[derive(Error, Debug)]
pub enum GetProfilePathError {
    #[error("Failed to deduce HOME directory for user {0}")]
    NoUserHome(String),
}

fn get_profile_path(
    profile_path: Option<String>,
    profile_user: Option<String>,
    profile_name: Option<String>,
) -> Result<String, GetProfilePathError> {
    match (profile_path, profile_user, profile_name) {
        (Some(profile_path), None, None) => Ok(profile_path),
        (None, Some(profile_user), Some(profile_name)) => {
            let nix_state_dir = env::var("NIX_STATE_DIR").unwrap_or("/nix/var/nix".to_string());
            // As per https://nixos.org/manual/nix/stable/command-ref/files/profiles#profiles
            match &profile_user[..] {
                "root" => {
                    match &profile_name[..] {
                        // NixOS system profile belongs to the root user, but isn't stored in the 'per-user/root'
                        "system" => Ok(format!("{}/profiles/system", nix_state_dir)),
                        _ => Ok(format!(
                            "{}/profiles/per-user/root/{}",
                            nix_state_dir, profile_name
                        )),
                    }
                }
                _ => {
                    let old_user_profiles_dir =
                        format!("{}/profiles/per-user/{}", nix_state_dir, profile_user);
                    // To stay backward compatible
                    if Path::new(&old_user_profiles_dir).exists() {
                        Ok(format!("{}/{}", old_user_profiles_dir, profile_name))
                    } else {
                        // https://github.com/NixOS/nix/blob/2.17.0/src/libstore/profiles.cc#L308
                        // This is basically the equivalent of calling 'dirs::state_dir()'.
                        // However, this function returns 'None' on macOS, while nix will actually
                        // check env variables, so we imitate nix implementation below instead of
                        // using 'dirs::state_dir()' directly.
                        let state_dir = env::var("XDG_STATE_HOME").or_else(|_| {
                            dirs::home_dir()
                                .map(|h| {
                                    format!("{}/.local/state", h.as_path().display().to_string())
                                })
                                .ok_or(GetProfilePathError::NoUserHome(profile_user))
                        })?;
                        Ok(format!("{}/nix/profiles/{}", state_dir, profile_name))
                    }
                }
            }
        }
        _ => panic!("impossible"),
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Ensure that this process stays alive after the SSH connection dies
    let mut signals = Signals::new(&[SIGHUP])?;
    std::thread::spawn(move || {
        for _ in signals.forever() {
            println!("Received SIGHUP - ignoring...");
        }
    });

    let opts: Opts = Opts::parse();

    deploy::init_logger(
        opts.debug_logs,
        opts.log_dir.as_deref(),
        &match opts.subcmd {
            SubCommand::Activate(_) => deploy::LoggerType::Activate,
            SubCommand::Wait(_) => deploy::LoggerType::Wait,
            SubCommand::Revoke(_) => deploy::LoggerType::Revoke,
        },
    )?;

    let r = match opts.subcmd {
        SubCommand::Activate(activate_opts) => activate(
            get_profile_path(
                activate_opts.profile_path,
                activate_opts.profile_user,
                activate_opts.profile_name,
            )?,
            activate_opts.closure,
            activate_opts.auto_rollback,
            activate_opts.temp_path,
            activate_opts.confirm_timeout,
            activate_opts.magic_rollback,
            activate_opts.dry_activate,
            activate_opts.boot,
        )
        .await
        .map_err(|x| Box::new(x) as Box<dyn std::error::Error>),

        SubCommand::Wait(wait_opts) => wait(wait_opts.temp_path, wait_opts.closure, wait_opts.activation_timeout)
            .await
            .map_err(|x| Box::new(x) as Box<dyn std::error::Error>),

        SubCommand::Revoke(revoke_opts) => revoke(get_profile_path(
            revoke_opts.profile_path,
            revoke_opts.profile_user,
            revoke_opts.profile_name,
        )?)
        .await
        .map_err(|x| Box::new(x) as Box<dyn std::error::Error>),
    };

    match r {
        Ok(()) => (),
        Err(err) => {
            error!("{}", err);
            std::process::exit(1)
        }
    }

    Ok(())
}
