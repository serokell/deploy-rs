// SPDX-FileCopyrightText: 2020 Serokell <https://serokell.io/>
// SPDX-FileCopyrightText: 2020 Andreas Fuchs <asf@boinkor.net>
// SPDX-FileCopyrightText: 2021 Yannik Sander <contact@ysndr.de>
//
// SPDX-License-Identifier: MPL-2.0

use signal_hook::{consts::signal::SIGHUP, iterator::Signals};

use clap::Parser;

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
#[derive(Parser, Debug)]
#[command(version = "1.0", author = "Serokell <https://serokell.io/>")]
struct Opts {
    /// Print debug logs to output
    #[arg(short, long)]
    debug_logs: bool,
    /// Directory to print logs to
    #[arg(long)]
    log_dir: Option<String>,

    #[command(subcommand)]
    subcmd: SubCommand,
}

#[derive(Parser, Debug)]
enum SubCommand {
    Activate(ActivateOpts),
    Wait(WaitOpts),
    Revoke(RevokeOpts),
}

/// Activate a profile
#[derive(Parser, Debug)]
#[command(group(
    clap::ArgGroup::new("profile")
        .required(true)
        .multiple(false)
        .args(&["profile_path","profile_user"])
))]
struct ActivateOpts {
    /// The closure to activate
    closure: String,
    /// The profile path to install into
    #[arg(long)]
    profile_path: Option<String>,
    /// The profile user if explicit profile path is not specified
    #[arg(long, requires = "profile_name")]
    profile_user: Option<String>,
    /// The profile name
    #[arg(long, requires = "profile_user")]
    profile_name: Option<String>,

    /// Maximum time to wait for confirmation after activation
    #[arg(long)]
    confirm_timeout: u16,

    /// Wait for confirmation after deployment and rollback if not confirmed
    #[arg(long)]
    magic_rollback: bool,

    /// Auto rollback if failure
    #[arg(long)]
    auto_rollback: bool,

    /// Show what will be activated on the machines
    #[arg(long)]
    dry_activate: bool,

    /// Don't activate, but update the boot loader to boot into the new profile
    #[arg(long)]
    boot: bool,

    /// Activate the configuration, but don't update the boot loader
    #[arg(long)]
    test: bool,

    /// Path for any temporary files that may be needed during activation
    #[arg(long)]
    temp_path: PathBuf,
}

/// Wait for profile activation
#[derive(Parser, Debug)]
struct WaitOpts {
    /// The closure to wait for
    closure: String,

    /// Path for any temporary files that may be needed during activation
    #[arg(long)]
    temp_path: PathBuf,

    /// Timeout to wait for activation
    #[arg(long)]
    activation_timeout: Option<u16>,
}

/// Revoke profile activation
#[derive(Parser, Debug)]
struct RevokeOpts {
    /// The profile path to install into
    #[arg(long)]
    profile_path: Option<String>,
    /// The profile user if explicit profile path is not specified
    #[arg(long, requires = "profile_name")]
    profile_user: Option<String>,
    /// The profile name
    #[arg(long, requires = "profile_user")]
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
        .arg(profile_path)
        .arg("--rollback");
    command::Command::new(nix_env_rollback_command)
        .status()
        .await
        .map_err(DeactivateError::Rollback)?;

    debug!("Listing generations");

    let mut nix_env_list_generations_command = Command::new("nix-env");
    nix_env_list_generations_command
        .arg("-p")
        .arg(profile_path)
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
        .arg(profile_path)
        .arg("--delete-generations")
        .arg(last_generation_id);
    command::Command::new(nix_env_delete_generation_command)
        .status()
        .await
        .map_err(DeactivateError::DeleteGen)?;

    info!("Attempting to re-activate the last generation");

    let mut re_activate_command = Command::new(format!("{}/deploy-rs-activate", profile_path));
    re_activate_command
        .env("PROFILE", profile_path)
        .current_dir(profile_path);
    command::Command::new(re_activate_command)
        .status()
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

#[derive(Debug)]
pub enum WaitEvent {
    Confirmed,
    Cancelled,
}

#[derive(Error, Debug)]
pub enum DangerZoneError {
    #[error("Timeout elapsed for confirmation")]
    TimesUp,
    #[error("inotify stream ended without activation confirmation")]
    NoConfirmation,
    #[error("inotify encountered an error: {0}")]
    Watch(notify::Error),
    #[error("Activation cancelled by deployment client")]
    Cancelled,
}

async fn danger_zone(
    mut events: mpsc::Receiver<Result<WaitEvent, notify::Error>>,
    confirm_timeout: u16,
) -> Result<(), DangerZoneError> {
    info!("Waiting for confirmation event...");

    match timeout(Duration::from_secs(confirm_timeout as u64), events.recv()).await {
        Ok(Some(Ok(WaitEvent::Confirmed))) => Ok(()),
        Ok(Some(Ok(WaitEvent::Cancelled))) => Err(DangerZoneError::Cancelled),
        Ok(Some(Err(e))) => Err(DangerZoneError::Watch(e)),
        Ok(None) => Err(DangerZoneError::NoConfirmation),
        Err(_) => Err(DangerZoneError::TimesUp),
    }
}

fn confirmation_watcher(
    temp_path: &Path,
    lock_path: &Path,
) -> Result<
    (
        RecommendedWatcher,
        mpsc::Receiver<Result<WaitEvent, notify::Error>>,
    ),
    notify::Error,
> {
    let (deleted, done) = mpsc::channel(1);
    let lock_path = lock_path.to_path_buf();

    let mut watcher =
        recommended_watcher(move |res: Result<notify::event::Event, notify::Error>| {
            let send_result = match res {
                Ok(e)
                    if e.kind == notify::EventKind::Remove(notify::event::RemoveKind::File)
                        && e.paths.iter().any(|path| path == &lock_path) =>
                {
                    debug!("Got canary removal event, sending on channel");
                    deleted.try_send(Ok(WaitEvent::Confirmed))
                }
                Err(e) => {
                    debug!("Got error waiting for removal event, sending on channel");
                    deleted.try_send(Err(e))
                }
                Ok(_) => Ok(()),
            };

            if let Err(e) = send_result {
                error!("Could not send file system event to watcher: {}", e);
            }
        })?;

    watcher.watch(temp_path, RecursiveMode::NonRecursive)?;

    Ok((watcher, done))
}

async fn create_activation_cancel(temp_path: &Path, closure: &str) {
    let cancel_path = deploy::make_cancel_path(temp_path, closure);

    debug!("Creating cancel file to signal wait process");

    if let Some(parent) = cancel_path.parent() {
        if let Err(e) = fs::create_dir_all(parent).await {
            debug!("Failed to create parent directory for cancel file: {}", e);
            return;
        }
    }

    if let Err(e) = fs::File::create(&cancel_path).await {
        debug!("Failed to create cancel file: {}", e);
    } else {
        debug!(
            "Cancel file created successfully at {}",
            cancel_path.display()
        );
    }
}

async fn remove_activation_cancel(temp_path: &Path, closure: &str) {
    let cancel_path = deploy::make_cancel_path(temp_path, closure);

    match fs::remove_file(&cancel_path).await {
        Ok(()) => debug!("Removed cancel file at {}", cancel_path.display()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => (),
        Err(e) => debug!("Failed to remove cancel file: {}", e),
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

    debug!("Creating notify watcher");

    let (_watcher, done) = confirmation_watcher(&temp_path, &lock_path)?;

    debug!("Creating canary file");

    fs::File::create(&lock_path)
        .await
        .map_err(ActivationConfirmationError::CreateConfirmFile)?;

    danger_zone(done, confirm_timeout)
        .await
        .map_err(ActivationConfirmationError::WaitingError)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static TEMP_DIRECTORY_ID: AtomicU64 = AtomicU64::new(0);

    #[tokio::test]
    async fn confirmation_watcher_observes_immediate_canary_removal() {
        let id = TEMP_DIRECTORY_ID.fetch_add(1, Ordering::Relaxed);
        let temp_path = env::temp_dir().join(format!(
            "deploy-rs-confirmation-test-{}-{id}",
            std::process::id()
        ));
        let lock_path = temp_path.join("canary");

        fs::create_dir_all(&temp_path)
            .await
            .expect("create test directory");
        let (_watcher, done) =
            confirmation_watcher(&temp_path, &lock_path).expect("create confirmation watcher");

        fs::File::create(&lock_path)
            .await
            .expect("create canary file");
        fs::remove_file(&lock_path)
            .await
            .expect("remove canary file");

        danger_zone(done, 1).await.expect("observe canary removal");
        fs::remove_dir(&temp_path)
            .await
            .expect("remove test directory");
    }

    fn test_closure(id: u64) -> String {
        format!("/nix/store/0000000000000000000000000000000cancel-test-{id}")
    }

    async fn test_directory(name: &str, id: u64) -> PathBuf {
        let temp_path =
            env::temp_dir().join(format!("deploy-rs-{name}-{}-{id}", std::process::id()));

        fs::create_dir_all(&temp_path)
            .await
            .expect("create test directory");

        temp_path
    }

    #[tokio::test]
    async fn wait_cancelled_when_cancel_file_already_exists() {
        let id = TEMP_DIRECTORY_ID.fetch_add(1, Ordering::Relaxed);
        let temp_path = env::temp_dir().join(format!(
            "deploy-rs-cancel-before-{}-{id}",
            std::process::id()
        ));
        let closure = test_closure(id);
        fs::create_dir_all(&temp_path)
            .await
            .expect("create test directory");

        // Cancel file exists before wait() is even called.
        create_activation_cancel(&temp_path, &closure).await;

        let wait_result = wait(temp_path.clone(), closure.clone(), Some(5)).await;
        match wait_result {
            Err(WaitError::Waiting(DangerZoneError::Cancelled)) => {}
            other => panic!("expected Cancelled, got {:?}", other),
        }

        let _ = fs::remove_dir_all(&temp_path).await;
    }

    #[tokio::test]
    async fn wait_cancelled_when_cancel_file_created_while_waiting() {
        let id = TEMP_DIRECTORY_ID.fetch_add(1, Ordering::Relaxed);
        let temp_path = test_directory("cancel-during", id).await;
        let closure = test_closure(id);

        let waiting = tokio::spawn(wait(temp_path.clone(), closure.clone(), Some(10)));

        // Let wait() get past its existence check, so that the sentinel is picked up
        // by the watcher rather than by the check.
        tokio::time::sleep(Duration::from_millis(200)).await;
        create_activation_cancel(&temp_path, &closure).await;

        match waiting.await.expect("join wait task") {
            Err(WaitError::Waiting(DangerZoneError::Cancelled)) => {}
            other => panic!("expected Cancelled, got {:?}", other),
        }

        assert!(
            !deploy::make_cancel_path(&temp_path, &closure).exists(),
            "wait should not leave the cancel file behind"
        );

        let _ = fs::remove_dir_all(&temp_path).await;
    }

    #[tokio::test]
    async fn wait_returns_when_canary_created_while_waiting() {
        let id = TEMP_DIRECTORY_ID.fetch_add(1, Ordering::Relaxed);
        let temp_path = test_directory("canary-during", id).await;
        let closure = test_closure(id);

        let waiting = tokio::spawn(wait(temp_path.clone(), closure.clone(), Some(10)));

        tokio::time::sleep(Duration::from_millis(200)).await;
        fs::File::create(deploy::make_lock_path(&temp_path, &closure))
            .await
            .expect("create canary file");

        waiting
            .await
            .expect("join wait task")
            .expect("observe canary creation");

        let _ = fs::remove_dir_all(&temp_path).await;
    }

    #[tokio::test]
    async fn remove_activation_cancel_clears_the_sentinel() {
        let id = TEMP_DIRECTORY_ID.fetch_add(1, Ordering::Relaxed);
        let temp_path = test_directory("cancel-removal", id).await;
        let closure = test_closure(id);
        let cancel_path = deploy::make_cancel_path(&temp_path, &closure);

        // Removing an absent sentinel is a no-op, not an error.
        remove_activation_cancel(&temp_path, &closure).await;

        create_activation_cancel(&temp_path, &closure).await;
        assert!(cancel_path.exists(), "cancel file should have been created");

        remove_activation_cancel(&temp_path, &closure).await;
        assert!(
            !cancel_path.exists(),
            "cancel file should have been removed"
        );

        let _ = fs::remove_dir_all(&temp_path).await;
    }
}

#[derive(Error, Debug)]
pub enum WaitError {
    #[error("Error creating watcher for activation: {0}")]
    Watcher(#[from] notify::Error),
    #[error("Error waiting for activation: {0}")]
    Waiting(#[from] DangerZoneError),
}
pub async fn wait(
    temp_path: PathBuf,
    closure: String,
    activation_timeout: Option<u16>,
) -> Result<(), WaitError> {
    let lock_path = deploy::make_lock_path(&temp_path, &closure);
    let cancel_path = deploy::make_cancel_path(&temp_path, &closure);

    let (created, done) = mpsc::channel(1);

    let mut watcher: RecommendedWatcher = {
        // TODO: fix wasteful clone
        let lock_path = lock_path.clone();
        let cancel_path = cancel_path.clone();

        recommended_watcher(move |res: Result<notify::event::Event, notify::Error>| {
            let send_result = match res {
                Ok(e) if e.kind == notify::EventKind::Create(notify::event::CreateKind::File) => {
                    match &e.paths[..] {
                        // neither path may exist yet when some other files are created
                        // in 'temp_path'
                        // x is already supposed to be canonical path
                        [x] => {
                            if lock_path.canonicalize().is_ok_and(|p| x == &p) {
                                created.try_send(Ok(WaitEvent::Confirmed))
                            } else if cancel_path.canonicalize().is_ok_and(|p| x == &p) {
                                created.try_send(Ok(WaitEvent::Cancelled))
                            } else {
                                Ok(())
                            }
                        }
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
    // 'activate' may have failed before the watcher above existed, so the sentinel
    // it leaves behind has to be picked up here rather than waited for.
    if fs::metadata(&cancel_path).await.is_ok() {
        watcher.unwatch(&temp_path)?;
        remove_activation_cancel(&temp_path, &closure).await;
        return Err(DangerZoneError::Cancelled.into());
    }

    let waited = danger_zone(done, activation_timeout.unwrap_or(240)).await;

    if matches!(waited, Err(DangerZoneError::Cancelled)) {
        remove_activation_cancel(&temp_path, &closure).await;
    }

    waited?;

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

#[allow(clippy::too_many_arguments)]
pub async fn activate(
    profile_path: String,
    closure: String,
    auto_rollback: bool,
    temp_path: PathBuf,
    confirm_timeout: u16,
    magic_rollback: bool,
    dry_activate: bool,
    boot: bool,
    test: bool,
) -> Result<(), ActivateError> {
    // The deployer only spawns a 'wait' process under these conditions, so they are
    // also the only ones under which there is anything to signal a cancellation to.
    let signal_cancellation = magic_rollback && !boot && !dry_activate;

    if signal_cancellation {
        remove_activation_cancel(&temp_path, &closure).await;
    }

    if !dry_activate {
        info!("Activating profile");
        let mut nix_env_set_command = Command::new("nix-env");
        nix_env_set_command
            .arg("-p")
            .arg(&profile_path)
            .arg("--set")
            .arg(&closure);
        let nix_env_set_exit_status = nix_env_set_command
            .status()
            .await
            .map_err(|err| ActivateError::SetProfile(command::CommandError::RunError(err)))?;
        match nix_env_set_exit_status.code() {
            Some(0) => (),
            _exit_code => {
                if signal_cancellation {
                    create_activation_cancel(&temp_path, &closure).await;
                }
                if auto_rollback && !dry_activate {
                    deactivate(&profile_path).await?;
                }
                return Err(ActivateError::SetProfile(
                    command::CommandError::ExitStatus(
                        nix_env_set_exit_status,
                        format!("{:?}", nix_env_set_command),
                    ),
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
        .env("TEST", if test { "1" } else { "0" })
        .current_dir(activation_location);
    let activate_status = match activate_command
        .status()
        .await
        .map_err(|err| ActivateError::RunActivate(command::CommandError::RunError(err)))
    {
        Ok(x) => x,
        Err(e) => {
            if signal_cancellation {
                create_activation_cancel(&temp_path, &closure).await;
            }
            if auto_rollback && !dry_activate {
                deactivate(&profile_path).await?;
            }
            return Err(e);
        }
    };

    if !dry_activate {
        match activate_status.code() {
            Some(0) => (),
            _exit_code => {
                if signal_cancellation {
                    create_activation_cancel(&temp_path, &closure).await;
                }
                if auto_rollback {
                    deactivate(&profile_path).await?;
                }
                return Err(ActivateError::RunActivate(
                    command::CommandError::ExitStatus(
                        activate_status,
                        format!("{:?}", activate_command),
                    ),
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
                                .map(|h| format!("{}/.local/state", h.as_path().display()))
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
    let mut signals = Signals::new([SIGHUP])?;
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
            activate_opts.test,
        )
        .await
        .map_err(|x| Box::new(x) as Box<dyn std::error::Error>),

        SubCommand::Wait(wait_opts) => wait(
            wait_opts.temp_path,
            wait_opts.closure,
            wait_opts.activation_timeout,
        )
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
