// SPDX-FileCopyrightText: 2020 Serokell <https://serokell.io/>
// SPDX-FileCopyrightText: 2020 Andreas Fuchs <asf@boinkor.net>
//
// SPDX-License-Identifier: MPL-2.0

use std::borrow::Cow;
use tokio::process::Command;

use thiserror::Error;

fn build_activate_command(
    sudo: &Option<String>,
    profile_path: &str,
    closure: &str,
    auto_rollback: bool,
    temp_path: &str,
    confirm_timeout: u16,
    magic_rollback: bool,
    debug_logs: bool,
    log_dir: Option<&str>,
) -> String {
    let mut self_activate_command = format!("{}/activate-rs", closure);

    if debug_logs {
        self_activate_command = format!("{} --debug-logs", self_activate_command);
    }

    if let Some(log_dir) = log_dir {
        self_activate_command = format!("{} --log-dir {}", self_activate_command, log_dir);
    }

    self_activate_command = format!(
        "{} --temp-path '{}' activate '{}' '{}'",
        self_activate_command, temp_path, closure, profile_path
    );

    self_activate_command = format!(
        "{} --confirm-timeout {}",
        self_activate_command, confirm_timeout
    );

    if magic_rollback {
        self_activate_command = format!("{} --magic-rollback", self_activate_command);
    }

    if auto_rollback {
        self_activate_command = format!("{} --auto-rollback", self_activate_command);
    }

    if let Some(sudo_cmd) = &sudo {
        self_activate_command = format!("{} {}", sudo_cmd, self_activate_command);
    }

    self_activate_command
}

#[test]
fn test_activation_command_builder() {
    let sudo = Some("sudo -u test".to_string());
    let profile_path = "/blah/profiles/test";
    let closure = "/nix/store/blah/etc";
    let auto_rollback = true;
    let temp_path = "/tmp";
    let confirm_timeout = 30;
    let magic_rollback = true;
    let debug_logs = true;
    let log_dir = Some("/tmp/something.txt");

    assert_eq!(
        build_activate_command(
            &sudo,
            profile_path,
            closure,
            auto_rollback,
            temp_path,
            confirm_timeout,
            magic_rollback,
            debug_logs,
            log_dir
        ),
        "sudo -u test /nix/store/blah/etc/activate-rs --debug-logs --log-dir /tmp/something.txt --temp-path '/tmp' activate '/nix/store/blah/etc' '/blah/profiles/test' --confirm-timeout 30 --magic-rollback --auto-rollback"
            .to_string(),
    );
}

fn build_wait_command(
    sudo: &Option<String>,
    closure: &str,
    temp_path: &str,
    debug_logs: bool,
    log_dir: Option<&str>,
) -> String {
    let mut self_activate_command = format!("{}/activate-rs", closure);

    if debug_logs {
        self_activate_command = format!("{} --debug-logs", self_activate_command);
    }

    if let Some(log_dir) = log_dir {
        self_activate_command = format!("{} --log-dir {}", self_activate_command, log_dir);
    }

    self_activate_command = format!(
        "{} --temp-path '{}' wait '{}'",
        self_activate_command, temp_path, closure
    );

    if let Some(sudo_cmd) = &sudo {
        self_activate_command = format!("{} {}", sudo_cmd, self_activate_command);
    }

    self_activate_command
}

#[test]
fn test_wait_command_builder() {
    let sudo = Some("sudo -u test".to_string());
    let closure = "/nix/store/blah/etc";
    let temp_path = "/tmp";
    let debug_logs = true;
    let log_dir = Some("/tmp/something.txt");

    assert_eq!(
        build_wait_command(
            &sudo,
            closure,
            temp_path,
            debug_logs,
            log_dir
        ),
        "sudo -u test /nix/store/blah/etc/activate-rs --debug-logs --log-dir /tmp/something.txt --temp-path '/tmp' wait '/nix/store/blah/etc'"
            .to_string(),
    );
}

#[derive(Error, Debug)]
pub enum DeployProfileError {
    #[error("Failed to calculate activate bin path from deploy bin path: {0}")]
    DeployPathToActivatePathError(#[from] super::DeployPathToActivatePathError),

    #[error("Failed to spawn activation command over SSH: {0}")]
    SSHSpawnActivateError(std::io::Error),

    #[error("Failed to run activation command over SSH: {0}")]
    SSHActivateError(std::io::Error),
    #[error("Activating over SSH resulted in a bad exit code: {0:?}")]
    SSHActivateExitError(Option<i32>),

    #[error("Failed to run wait command over SSH: {0}")]
    SSHWaitError(std::io::Error),
    #[error("Waiting over SSH resulted in a bad exit code: {0:?}")]
    SSHWaitExitError(Option<i32>),

    #[error("Failed to run confirmation command over SSH (the server should roll back): {0}")]
    SSHConfirmError(std::io::Error),
    #[error(
        "Confirming activation over SSH resulted in a bad exit code (the server should roll back): {0:?}"
    )]
    SSHConfirmExitError(Option<i32>),
}

pub async fn deploy_profile(
    deploy_data: &super::DeployData<'_>,
    deploy_defs: &super::DeployDefs,
) -> Result<(), DeployProfileError> {
    info!(
        "Activating profile `{}` for node `{}`",
        deploy_data.profile_name, deploy_data.node_name
    );

    let temp_path: Cow<str> = match &deploy_data.merged_settings.temp_path {
        Some(x) => x.into(),
        None => "/tmp".into(),
    };

    let confirm_timeout = deploy_data.merged_settings.confirm_timeout.unwrap_or(30);

    let magic_rollback = deploy_data.merged_settings.magic_rollback.unwrap_or(true);

    let auto_rollback = deploy_data.merged_settings.auto_rollback.unwrap_or(true);

    let self_activate_command = build_activate_command(
        &deploy_defs.sudo,
        &deploy_defs.profile_path,
        &deploy_data.profile.profile_settings.path,
        auto_rollback,
        &temp_path,
        confirm_timeout,
        magic_rollback,
        deploy_data.debug_logs,
        deploy_data.log_dir,
    );

    debug!("Constructed activation command: {}", self_activate_command);

    let self_wait_command = build_wait_command(
        &deploy_defs.sudo,
        &deploy_data.profile.profile_settings.path,
        &temp_path,
        deploy_data.debug_logs,
        deploy_data.log_dir,
    );

    debug!("Constructed wait command: {}", self_wait_command);

    let hostname = match deploy_data.cmd_overrides.hostname {
        Some(ref x) => x,
        None => &deploy_data.node.node_settings.hostname,
    };

    let ssh_addr = format!("ssh://{}@{}", deploy_defs.ssh_user, hostname);

    let mut ssh_activate_command_ = Command::new("ssh");
    let ssh_activate_command = ssh_activate_command_.arg(&ssh_addr);

    for ssh_opt in &deploy_data.merged_settings.ssh_opts {
        ssh_activate_command.arg(&ssh_opt);
    }

    if !magic_rollback {
        let ssh_activate_exit_status = ssh_activate_command
            .arg(self_activate_command)
            .status()
            .await
            .map_err(DeployProfileError::SSHActivateError)?;

        match ssh_activate_exit_status.code() {
            Some(0) => (),
            a => return Err(DeployProfileError::SSHActivateExitError(a)),
        };

        info!("Success activating, done!");
    } else {
        let ssh_activate = ssh_activate_command
            .arg(self_activate_command)
            .spawn()
            .map_err(DeployProfileError::SSHSpawnActivateError)?;

        info!("Creating activation waiter");

        let mut ssh_wait_command_ = Command::new("ssh");
        let ssh_wait_command = ssh_wait_command_.arg(&ssh_addr);

        for ssh_opt in &deploy_data.merged_settings.ssh_opts {
            ssh_wait_command.arg(ssh_opt);
        }

        let ssh_wait_exit_status = ssh_wait_command
            .arg(self_wait_command)
            .status()
            .await
            .map_err(DeployProfileError::SSHWaitError)?;

        match ssh_wait_exit_status.code() {
            Some(0) => (),
            a => return Err(DeployProfileError::SSHWaitExitError(a)),
        };

        info!("Success activating, attempting to confirm activation");

        let mut c = Command::new("ssh");
        let mut ssh_confirm_command = c.arg(format!("ssh://{}@{}", deploy_defs.ssh_user, hostname));

        for ssh_opt in &deploy_data.merged_settings.ssh_opts {
            ssh_confirm_command = ssh_confirm_command.arg(ssh_opt);
        }

        let lock_path =
            super::make_lock_path(&temp_path, &deploy_data.profile.profile_settings.path);

        let mut confirm_command = format!("rm {}", lock_path);
        if let Some(sudo_cmd) = &deploy_defs.sudo {
            confirm_command = format!("{} {}", sudo_cmd, confirm_command);
        }

        debug!(
            "Attempting to run command to confirm deployment: {}",
            confirm_command
        );

        let ssh_exit_status = ssh_confirm_command
            .arg(confirm_command)
            .status()
            .await
            .map_err(DeployProfileError::SSHConfirmError)?;

        match ssh_exit_status.code() {
            Some(0) => (),
            a => return Err(DeployProfileError::SSHConfirmExitError(a)),
        };

        info!("Deployment confirmed.");
    }

    Ok(())
}
