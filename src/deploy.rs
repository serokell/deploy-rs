// SPDX-FileCopyrightText: 2020 Serokell <https://serokell.io/>
// SPDX-FileCopyrightText: 2020 Andreas Fuchs <asf@boinkor.net>
// SPDX-FileCopyrightText: 2021 Yannik Sander <contact@ysndr.de>
//
// SPDX-License-Identifier: MPL-2.0

use log::{debug, info};
use std::borrow::Cow;
use thiserror::Error;
use tokio::process::Command;

use crate::DeployDataDefsError;

struct ActivateCommandData<'a> {
    sudo: &'a Option<String>,
    profile_path: &'a str,
    closure: &'a str,
    auto_rollback: bool,
    temp_path: &'a str,
    confirm_timeout: u16,
    magic_rollback: bool,
    debug_logs: bool,
    log_dir: Option<&'a str>,
    dry_activate: bool,
}

fn build_activate_command(data: &ActivateCommandData) -> String {
    let mut self_activate_command = format!("{}/activate-rs", data.closure);

    if data.debug_logs {
        self_activate_command = format!("{} --debug-logs", self_activate_command);
    }

    if let Some(log_dir) = data.log_dir {
        self_activate_command = format!("{} --log-dir {}", self_activate_command, log_dir);
    }

    self_activate_command = format!(
        "{} activate '{}' '{}' --temp-path '{}'",
        self_activate_command, data.closure, data.profile_path, data.temp_path
    );

    self_activate_command = format!(
        "{} --confirm-timeout {}",
        self_activate_command, data.confirm_timeout
    );

    if data.magic_rollback {
        self_activate_command = format!("{} --magic-rollback", self_activate_command);
    }

    if data.auto_rollback {
        self_activate_command = format!("{} --auto-rollback", self_activate_command);
    }

    if data.dry_activate {
        self_activate_command = format!("{} --dry-activate", self_activate_command);
    }

    if let Some(sudo_cmd) = &data.sudo {
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
    let dry_activate = false;
    let temp_path = "/tmp";
    let confirm_timeout = 30;
    let magic_rollback = true;
    let debug_logs = true;
    let log_dir = Some("/tmp/something.txt");

    assert_eq!(
        build_activate_command(&ActivateCommandData {
            sudo: &sudo,
            profile_path,
            closure,
            auto_rollback,
            temp_path,
            confirm_timeout,
            magic_rollback,
            debug_logs,
            log_dir,
            dry_activate
        }),
        "sudo -u test /nix/store/blah/etc/activate-rs --debug-logs --log-dir /tmp/something.txt activate '/nix/store/blah/etc' '/blah/profiles/test' --temp-path '/tmp' --confirm-timeout 30 --magic-rollback --auto-rollback"
            .to_string(),
    );
}

struct WaitCommandData<'a> {
    sudo: &'a Option<String>,
    closure: &'a str,
    temp_path: &'a str,
    debug_logs: bool,
    log_dir: Option<&'a str>,
}

fn build_wait_command(data: &WaitCommandData) -> String {
    let mut self_activate_command = format!("{}/activate-rs", data.closure);

    if data.debug_logs {
        self_activate_command = format!("{} --debug-logs", self_activate_command);
    }

    if let Some(log_dir) = data.log_dir {
        self_activate_command = format!("{} --log-dir {}", self_activate_command, log_dir);
    }

    self_activate_command = format!(
        "{} wait '{}' --temp-path '{}'",
        self_activate_command, data.closure, data.temp_path,
    );

    if let Some(sudo_cmd) = &data.sudo {
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
        build_wait_command(&WaitCommandData {
            sudo: &sudo,
            closure,
            temp_path,
            debug_logs,
            log_dir
        }),
        "sudo -u test /nix/store/blah/etc/activate-rs --debug-logs --log-dir /tmp/something.txt wait '/nix/store/blah/etc' --temp-path '/tmp'"
            .to_string(),
    );
}

struct RevokeCommandData<'a> {
    sudo: &'a Option<String>,
    closure: &'a str,
    profile_path: &'a str,
    debug_logs: bool,
    log_dir: Option<&'a str>,
}

fn build_revoke_command(data: &RevokeCommandData) -> String {
    let mut self_activate_command = format!("{}/activate-rs", data.closure);

    if data.debug_logs {
        self_activate_command = format!("{} --debug-logs", self_activate_command);
    }

    if let Some(log_dir) = data.log_dir {
        self_activate_command = format!("{} --log-dir {}", self_activate_command, log_dir);
    }

    self_activate_command = format!("{} revoke '{}'", self_activate_command, data.profile_path);

    if let Some(sudo_cmd) = &data.sudo {
        self_activate_command = format!("{} {}", sudo_cmd, self_activate_command);
    }

    self_activate_command
}

#[test]
fn test_revoke_command_builder() {
    let sudo = Some("sudo -u test".to_string());
    let closure = "/nix/store/blah/etc";
    let profile_path = "/nix/var/nix/per-user/user/profile";
    let debug_logs = true;
    let log_dir = Some("/tmp/something.txt");

    assert_eq!(
        build_revoke_command(&RevokeCommandData {
            sudo: &sudo,
            closure,
            profile_path,
            debug_logs,
            log_dir
        }),
        "sudo -u test /nix/store/blah/etc/activate-rs --debug-logs --log-dir /tmp/something.txt revoke '/nix/var/nix/per-user/user/profile'"
            .to_string(),
    );
}

#[derive(Error, Debug)]
pub enum ConfirmProfileError {
    #[error("Failed to run confirmation command over SSH (the server should roll back): {0}")]
    SSHConfirm(std::io::Error),
    #[error(
        "Confirming activation over SSH resulted in a bad exit code (the server should roll back): {0:?}"
    )]
    SSHConfirmExit(Option<i32>),
}

pub async fn confirm_profile(
    deploy_data: &super::DeployData<'_>,
    deploy_defs: &super::DeployDefs,
    temp_path: Cow<'_, str>,
    ssh_addr: &str,
) -> Result<(), ConfirmProfileError> {
    let mut ssh_confirm_command = Command::new("ssh");
    ssh_confirm_command.arg(ssh_addr);

    for ssh_opt in &deploy_data.merged_settings.ssh_opts {
        ssh_confirm_command.arg(ssh_opt);
    }

    let lock_path = super::make_lock_path(&temp_path, &deploy_data.profile.profile_settings.path);

    let mut confirm_command = format!("rm {}", lock_path);
    if let Some(sudo_cmd) = &deploy_defs.sudo {
        confirm_command = format!("{} {}", sudo_cmd, confirm_command);
    }

    debug!(
        "Attempting to run command to confirm deployment: {}",
        confirm_command
    );

    let ssh_confirm_exit_status = ssh_confirm_command
        .arg(confirm_command)
        .status()
        .await
        .map_err(ConfirmProfileError::SSHConfirm)?;

    match ssh_confirm_exit_status.code() {
        Some(0) => (),
        a => return Err(ConfirmProfileError::SSHConfirmExit(a)),
    };

    info!("Deployment confirmed.");

    Ok(())
}

#[derive(Error, Debug)]
pub enum DeployProfileError {
    #[error("Failed to spawn activation command over SSH: {0}")]
    SSHSpawnActivate(std::io::Error),

    #[error("Failed to run activation command over SSH: {0}")]
    SSHActivate(std::io::Error),
    #[error("Activating over SSH resulted in a bad exit code: {0:?}")]
    SSHActivateExit(Option<i32>),

    #[error("Failed to run wait command over SSH: {0}")]
    SSHWait(std::io::Error),
    #[error("Waiting over SSH resulted in a bad exit code: {0:?}")]
    SSHWaitExit(Option<i32>),

    #[error("Error confirming deployment: {0}")]
    Confirm(#[from] ConfirmProfileError),
}

pub async fn deploy_profile(
    deploy_data: &super::DeployData<'_>,
    deploy_defs: &super::DeployDefs,
    dry_activate: bool,
) -> Result<(), DeployProfileError> {
    if !dry_activate {
        info!(
            "Activating profile `{}` for node `{}`",
            deploy_data.profile_name, deploy_data.node_name
        );
    }

    let temp_path: Cow<str> = match &deploy_data.merged_settings.temp_path {
        Some(x) => x.into(),
        None => "/tmp".into(),
    };

    let confirm_timeout = deploy_data.merged_settings.confirm_timeout.unwrap_or(30);

    let magic_rollback = deploy_data.merged_settings.magic_rollback.unwrap_or(true);

    let auto_rollback = deploy_data.merged_settings.auto_rollback.unwrap_or(true);

    let self_activate_command = build_activate_command(&ActivateCommandData {
        sudo: &deploy_defs.sudo,
        profile_path: &deploy_defs.profile_path,
        closure: &deploy_data.profile.profile_settings.path,
        auto_rollback,
        temp_path: &temp_path,
        confirm_timeout,
        magic_rollback,
        debug_logs: deploy_data.debug_logs,
        log_dir: deploy_data.log_dir,
        dry_activate,
    });

    debug!("Constructed activation command: {}", self_activate_command);

    let hostname = match deploy_data.cmd_overrides.hostname {
        Some(ref x) => x,
        None => &deploy_data.node.node_settings.hostname,
    };

    let ssh_addr = format!("{}@{}", deploy_defs.ssh_user, hostname);

    let mut ssh_activate_command = Command::new("ssh");
    ssh_activate_command.arg(&ssh_addr);

    for ssh_opt in &deploy_data.merged_settings.ssh_opts {
        ssh_activate_command.arg(&ssh_opt);
    }

    if !magic_rollback || dry_activate {
        let ssh_activate_exit_status = ssh_activate_command
            .arg(self_activate_command)
            .status()
            .await
            .map_err(DeployProfileError::SSHActivate)?;

        match ssh_activate_exit_status.code() {
            Some(0) => (),
            a => return Err(DeployProfileError::SSHActivateExit(a)),
        };

        if dry_activate {
            info!("Completed dry-activate!");
        } else {
            info!("Success activating, done!");
        }
    } else {
        let self_wait_command = build_wait_command(&WaitCommandData {
            sudo: &deploy_defs.sudo,
            closure: &deploy_data.profile.profile_settings.path,
            temp_path: &temp_path,
            debug_logs: deploy_data.debug_logs,
            log_dir: deploy_data.log_dir,
        });

        debug!("Constructed wait command: {}", self_wait_command);

        let ssh_activate = ssh_activate_command
            .arg(self_activate_command)
            .spawn()
            .map_err(DeployProfileError::SSHSpawnActivate)?;

        info!("Creating activation waiter");

        let mut ssh_wait_command = Command::new("ssh");
        ssh_wait_command.arg(&ssh_addr);

        for ssh_opt in &deploy_data.merged_settings.ssh_opts {
            ssh_wait_command.arg(ssh_opt);
        }

        let (send_activate, recv_activate) = tokio::sync::oneshot::channel();
        let (send_activated, recv_activated) = tokio::sync::oneshot::channel();

        let thread = tokio::spawn(async move {
            let o = ssh_activate.wait_with_output().await;

            let maybe_err = match o {
                Err(x) => Some(DeployProfileError::SSHActivate(x)),
                Ok(ref x) => match x.status.code() {
                    Some(0) => None,
                    a => Some(DeployProfileError::SSHActivateExit(a)),
                },
            };

            if let Some(err) = maybe_err {
                send_activate.send(err).unwrap();
            }

            send_activated.send(()).unwrap();
        });
        tokio::select! {
            x = ssh_wait_command.arg(self_wait_command).status() => {
                debug!("Wait command ended");
                match x.map_err(DeployProfileError::SSHWait)?.code() {
                    Some(0) => (),
                    a => return Err(DeployProfileError::SSHWaitExit(a)),
                };
            },
            x = recv_activate => {
                debug!("Activate command exited with an error");
                return Err(x.unwrap());
            },
        }

        info!("Success activating, attempting to confirm activation");

        let c = confirm_profile(deploy_data, deploy_defs, temp_path, &ssh_addr).await;
        recv_activated.await.unwrap();
        c?;

        thread
            .await
            .map_err(|x| DeployProfileError::SSHActivate(x.into()))?;
    }

    Ok(())
}

#[derive(Error, Debug)]
pub enum RevokeProfileError {
    #[error("Failed to spawn revocation command over SSH: {0}")]
    SSHSpawnRevoke(std::io::Error),

    #[error("Error revoking deployment: {0}")]
    SSHRevoke(std::io::Error),
    #[error("Revoking over SSH resulted in a bad exit code: {0:?}")]
    SSHRevokeExit(Option<i32>),

    #[error("Deployment data invalid: {0}")]
    InvalidDeployDataDefs(#[from] DeployDataDefsError),
}
pub async fn revoke(
    deploy_data: &crate::DeployData<'_>,
    deploy_defs: &crate::DeployDefs,
) -> Result<(), RevokeProfileError> {
    let self_revoke_command = build_revoke_command(&RevokeCommandData {
        sudo: &deploy_defs.sudo,
        closure: &deploy_data.profile.profile_settings.path,
        profile_path: &deploy_data.get_profile_path()?,
        debug_logs: deploy_data.debug_logs,
        log_dir: deploy_data.log_dir,
    });

    debug!("Constructed revoke command: {}", self_revoke_command);

    let hostname = match deploy_data.cmd_overrides.hostname {
        Some(ref x) => x,
        None => &deploy_data.node.node_settings.hostname,
    };

    let ssh_addr = format!("{}@{}", deploy_defs.ssh_user, hostname);

    let mut ssh_activate_command = Command::new("ssh");
    ssh_activate_command.arg(&ssh_addr);

    for ssh_opt in &deploy_data.merged_settings.ssh_opts {
        ssh_activate_command.arg(&ssh_opt);
    }

    let ssh_revoke = ssh_activate_command
        .arg(self_revoke_command)
        .spawn()
        .map_err(RevokeProfileError::SSHSpawnRevoke)?;

    let result = ssh_revoke.wait_with_output().await;

    match result {
        Err(x) => Err(RevokeProfileError::SSHRevoke(x)),
        Ok(ref x) => match x.status.code() {
            Some(0) => Ok(()),
            a => Err(RevokeProfileError::SSHRevokeExit(a)),
        },
    }
}
