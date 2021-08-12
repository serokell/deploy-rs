// SPDX-FileCopyrightText: 2020 Serokell <https://serokell.io/>
// SPDX-FileCopyrightText: 2020 Andreas Fuchs <asf@boinkor.net>
// SPDX-FileCopyrightText: 2021 Yannik Sander <contact@ysndr.de>
//
// SPDX-License-Identifier: MPL-2.0

use log::{debug, info};
use thiserror::Error;
use tokio::process::Command;

use crate::data;

pub struct SshCommand<'a> {
    hoststring: String,
    opts: &'a Vec<String>,
}

impl<'a> SshCommand<'a> {
    pub fn from_data(d: &'a data::DeployData) -> Result<Self, data::DeployDataError> {
        let hostname = match d.hostname {
            Some(x) => x,
            None => &d.node.node_settings.hostname,
        };
        let hoststring = format!("{}@{}", &d.ssh_user, hostname);
        let opts = d.merged_settings.ssh_opts.as_ref();
        Ok(SshCommand {hoststring, opts})
    }

    fn build(&self) -> Command {
        let mut cmd = Command::new("ssh");
        cmd.arg(&self.hoststring);
        cmd.args(self.opts.iter());
        cmd
    }
}

pub struct ActivateCommand<'a> {
    sudo: Option<&'a str>,
    profile_path: &'a str,
    temp_path: &'a str,
    closure: &'a str,
    auto_rollback: bool,
    confirm_timeout: u16,
    magic_rollback: bool,
    debug_logs: bool,
    log_dir: Option<&'a str>,
    dry_activate: bool,
}

impl<'a> ActivateCommand<'a> {
    pub fn from_data(d: &'a data::DeployData) -> Self {
        ActivateCommand {
            sudo: d.sudo.as_deref(),
            profile_path: &d.profile_path,
            temp_path: &d.temp_path,
            closure: &d.profile.profile_settings.path,
            auto_rollback: d.merged_settings.auto_rollback.unwrap_or(true),
            confirm_timeout: d.merged_settings.confirm_timeout.unwrap_or(30),
            magic_rollback: d.merged_settings.magic_rollback.unwrap_or(true),
            debug_logs: d.flags.debug_logs,
            log_dir: d.flags.log_dir.as_deref(),
            dry_activate: d.flags.dry_activate,
        }
    }

    fn build(self) -> String {
        let mut cmd = format!("{}/activate-rs", self.closure);

        if self.debug_logs {
            cmd = format!("{} --debug-logs", cmd);
        }

        if let Some(log_dir) = self.log_dir {
            cmd = format!("{} --log-dir {}", cmd, log_dir);
        }

        cmd = format!(
            "{} activate '{}' '{}' --temp-path '{}'",
            cmd, self.closure, self.profile_path, self.temp_path
        );

        cmd = format!(
            "{} --confirm-timeout {}",
            cmd, self.confirm_timeout
        );

        if self.magic_rollback {
            cmd = format!("{} --magic-rollback", cmd);
        }

        if self.auto_rollback {
            cmd = format!("{} --auto-rollback", cmd);
        }

        if self.dry_activate {
            cmd = format!("{} --dry-activate", cmd);
        }

        if let Some(sudo_cmd) = &self.sudo {
            cmd = format!("{} {}", sudo_cmd, cmd);
        }

        cmd
    }
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
        ActivateCommand {
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
        }.build(),
        "sudo -u test /nix/store/blah/etc/activate-rs --debug-logs --log-dir /tmp/something.txt activate '/nix/store/blah/etc' '/blah/profiles/test' --temp-path '/tmp' --confirm-timeout 30 --magic-rollback --auto-rollback"
            .to_string(),
    );
}

pub struct WaitCommand<'a> {
    sudo: Option<&'a str>,
    closure: &'a str,
    temp_path: &'a str,
    debug_logs: bool,
    log_dir: Option<&'a str>,
}

impl<'a> WaitCommand<'a> {
    pub fn from_data(d: &'a data::DeployData) -> Self {
        WaitCommand {
            sudo: d.sudo.as_deref(),
            temp_path: &d.temp_path,
            closure: &d.profile.profile_settings.path,
            debug_logs: d.flags.debug_logs,
            log_dir: d.flags.log_dir.as_deref(),
        }
    }

    fn build(self) -> String {
        let mut cmd = format!("{}/activate-rs", self.closure);

        if self.debug_logs {
            cmd = format!("{} --debug-logs", cmd);
        }

        if let Some(log_dir) = self.log_dir {
            cmd = format!("{} --log-dir {}", cmd, log_dir);
        }

        cmd = format!(
            "{} wait '{}' --temp-path '{}'",
            cmd, self.closure, self.temp_path,
        );

        if let Some(sudo_cmd) = &self.sudo {
            cmd = format!("{} {}", sudo_cmd, cmd);
        }

        cmd
    }
}

#[test]
fn test_wait_command_builder() {
    let sudo = Some("sudo -u test".to_string());
    let closure = "/nix/store/blah/etc";
    let temp_path = "/tmp";
    let debug_logs = true;
    let log_dir = Some("/tmp/something.txt");

    assert_eq!(
        WaitCommand {
            sudo: &sudo,
            closure,
            temp_path,
            debug_logs,
            log_dir
        }.build(),
        "sudo -u test /nix/store/blah/etc/activate-rs --debug-logs --log-dir /tmp/something.txt wait '/nix/store/blah/etc' --temp-path '/tmp'"
            .to_string(),
    );
}

pub struct RevokeCommand<'a> {
    sudo: Option<&'a str>,
    closure: &'a str,
    profile_path: &'a str,
    debug_logs: bool,
    log_dir: Option<&'a str>,
}

impl<'a> RevokeCommand<'a> {
    pub fn from_data(d: &'a data::DeployData) -> Self {
        RevokeCommand {
            sudo: d.sudo.as_deref(),
            profile_path: &d.profile_path,
            closure: &d.profile.profile_settings.path,
            debug_logs: d.flags.debug_logs,
            log_dir: d.flags.log_dir.as_deref(),
        }
    }


    fn build(self) -> String {
        let mut cmd = format!("{}/activate-rs", self.closure);

        if self.debug_logs {
            cmd = format!("{} --debug-logs", cmd);
        }

        if let Some(log_dir) = self.log_dir {
            cmd = format!("{} --log-dir {}", cmd, log_dir);
        }

        cmd = format!("{} revoke '{}'", cmd, self.profile_path);

        if let Some(sudo_cmd) = &self.sudo {
            cmd = format!("{} {}", sudo_cmd, cmd);
        }

        cmd
    }
}

#[test]
fn test_revoke_command_builder() {
    let sudo = Some("sudo -u test".to_string());
    let closure = "/nix/store/blah/etc";
    let profile_path = "/nix/var/nix/per-user/user/profile";
    let debug_logs = true;
    let log_dir = Some("/tmp/something.txt");

    assert_eq!(
        RevokeCommandData {
            sudo: &sudo,
            closure,
            profile_path,
            debug_logs,
            log_dir
        }.build(),
        "sudo -u test /nix/store/blah/etc/activate-rs --debug-logs --log-dir /tmp/something.txt revoke '/nix/var/nix/per-user/user/profile'"
            .to_string(),
    );
}

pub struct ConfirmCommand<'a> {
    sudo: Option<&'a str>,
    temp_path: &'a str,
    closure: &'a str,
}

impl<'a> ConfirmCommand<'a> {
    pub fn from_data(d: &'a data::DeployData) -> Self {
        ConfirmCommand {
            sudo: d.sudo.as_deref(),
            temp_path: &d.temp_path,
            closure: &d.profile.profile_settings.path,
        }
    }


    fn build(self) -> String {
        let lock_path = super::make_lock_path(&self.temp_path, &self.closure);

        let mut cmd = format!("rm {}", lock_path);
        if let Some(sudo_cmd) = &self.sudo {
            cmd = format!("{} {}", sudo_cmd, cmd);
        }
        cmd
    }
}

#[derive(Error, Debug)]
pub enum ConfirmProfileError {
    #[error("Failed to run confirmation command over SSH (the server should roll back): {0}")]
    SSHConfirmError(std::io::Error),
    #[error(
        "Confirming activation over SSH resulted in a bad exit code (the server should roll back): {0:?}"
    )]
    SSHConfirmExitError(Option<i32>),
}

pub async fn confirm_profile(
    ssh: SshCommand<'_>,
    confirm: ConfirmCommand<'_>,
) -> Result<(), ConfirmProfileError> {

    let mut ssh_confirm_cmd = ssh.build();

    let confirm_cmd = confirm.build();

    debug!(
        "Attempting to run command to confirm deployment: {}",
        confirm_cmd
    );

    let ssh_confirm_exit_status = ssh_confirm_cmd
        .arg(confirm_cmd)
        .status()
        .await
        .map_err(ConfirmProfileError::SSHConfirmError)?;

    match ssh_confirm_exit_status.code() {
        Some(0) => (),
        a => return Err(ConfirmProfileError::SSHConfirmExitError(a)),
    };

    info!("Deployment confirmed.");

    Ok(())
}

#[derive(Error, Debug)]
pub enum DeployProfileError {
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

    #[error("Error confirming deployment: {0}")]
    ConfirmError(#[from] ConfirmProfileError),
}

pub async fn deploy_profile(
    node_name: &str,
    profile_name: &str,
    ssh: SshCommand<'_>,
    activate: ActivateCommand<'_>,
    wait: WaitCommand<'_>,
    confirm: ConfirmCommand<'_>,
) -> Result<(), DeployProfileError> {
    if !activate.dry_activate {
        info!("Activating profile `{}` for node `{}`", profile_name, node_name);
    }
    let dry_activate = &activate.dry_activate.clone();
    let magic_rollback = &activate.magic_rollback.clone();

    let activate_cmd = activate.build();

    debug!("Constructed activation command: {}", activate_cmd);

    let mut ssh_activate_cmd = ssh.build();

    if !*magic_rollback || *dry_activate {
        let ssh_activate_exit_status = ssh_activate_cmd
            .arg(activate_cmd)
            .status()
            .await
            .map_err(DeployProfileError::SSHActivateError)?;

        match ssh_activate_exit_status.code() {
            Some(0) => (),
            a => return Err(DeployProfileError::SSHActivateExitError(a)),
        };

        if *dry_activate {
            info!("Completed dry-activate!");
        } else {
            info!("Success activating, done!");
        }
    } else {
        let wait_cmd = wait.build();

        debug!("Constructed wait command: {}", wait_cmd);

        let ssh_activate = ssh_activate_cmd
            .arg(activate_cmd)
            .spawn()
            .map_err(DeployProfileError::SSHSpawnActivateError)?;

        info!("Creating activation waiter");


        let mut ssh_wait_cmd = ssh.build();

        let (send_activate, recv_activate) = tokio::sync::oneshot::channel();
        let (send_activated, recv_activated) = tokio::sync::oneshot::channel();

        tokio::spawn(async move {
            let o = ssh_activate.wait_with_output().await;

            let maybe_err = match o {
                Err(x) => Some(DeployProfileError::SSHActivateError(x)),
                Ok(ref x) => match x.status.code() {
                    Some(0) => None,
                    a => Some(DeployProfileError::SSHActivateExitError(a)),
                },
            };

            if let Some(err) = maybe_err {
                send_activate.send(err).unwrap();
            }

            send_activated.send(()).unwrap();
        });
        tokio::select! {
            x = ssh_wait_cmd.arg(wait_cmd).status() => {
                debug!("Wait command ended");
                match x.map_err(DeployProfileError::SSHWaitError)?.code() {
                    Some(0) => (),
                    a => return Err(DeployProfileError::SSHWaitExitError(a)),
                };
            },
            x = recv_activate => {
                debug!("Activate command exited with an error");
                return Err(x.unwrap());
            },
        }

        info!("Success activating, attempting to confirm activation");

        let c = confirm_profile(ssh, confirm).await;
        recv_activated.await.unwrap();
        c?;
    }

    Ok(())
}

#[derive(Error, Debug)]
pub enum RevokeProfileError {
    #[error("Failed to spawn revocation command over SSH: {0}")]
    SSHSpawnRevokeError(std::io::Error),

    #[error("Error revoking deployment: {0}")]
    SSHRevokeError(std::io::Error),
    #[error("Revoking over SSH resulted in a bad exit code: {0:?}")]
    SSHRevokeExitError(Option<i32>),
}
pub async fn revoke(
    node_name: &str,
    profile_name: &str,
    ssh: SshCommand<'_>,
    revoke: RevokeCommand<'_>,
) -> Result<(), RevokeProfileError> {
    info!("Revoking profile `{}` for node `{}`", profile_name, node_name);

    let revoke_cmd = revoke.build();
    debug!("Constructed revoke command: {}", revoke_cmd);

    let mut ssh_revoke_cmd = ssh.build();

    let ssh_revoke_cmd = ssh_revoke_cmd
        .arg(revoke_cmd)
        .spawn()
        .map_err(RevokeProfileError::SSHSpawnRevokeError)?;

    let result = ssh_revoke_cmd.wait_with_output().await;

    match result {
        Err(x) => Err(RevokeProfileError::SSHRevokeError(x)),
        Ok(ref x) => match x.status.code() {
            Some(0) => Ok(()),
            a => Err(RevokeProfileError::SSHRevokeExitError(a)),
        },
    }
}
