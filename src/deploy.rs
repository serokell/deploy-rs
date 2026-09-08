// SPDX-FileCopyrightText: 2020 Serokell <https://serokell.io/>
// SPDX-FileCopyrightText: 2020 Andreas Fuchs <asf@boinkor.net>
// SPDX-FileCopyrightText: 2021 Yannik Sander <contact@ysndr.de>
//
// SPDX-License-Identifier: MPL-2.0

use log::{debug, info, trace, warn};
use std::path::Path;
use thiserror::Error;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    process::Command,
};

use crate::{command, DeployDataDefsError, DeployDefs, ProfileInfo};

struct ActivateCommandData<'a> {
    sudo: &'a Option<String>,
    profile_info: &'a ProfileInfo,
    closure: &'a str,
    auto_rollback: bool,
    temp_path: &'a Path,
    confirm_timeout: u16,
    magic_rollback: bool,
    debug_logs: bool,
    log_dir: Option<&'a str>,
    dry_activate: bool,
    boot: bool,
    test: bool,
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
        "{} activate '{}' {} --temp-path '{}'",
        self_activate_command,
        data.closure,
        match data.profile_info {
            ProfileInfo::ProfilePath { profile_path } =>
                format!("--profile-path '{}'", profile_path),
            ProfileInfo::ProfileUserAndName {
                profile_user,
                profile_name,
            } => format!(
                "--profile-user {} --profile-name {}",
                profile_user, profile_name
            ),
        },
        data.temp_path.display()
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

    if data.boot {
        self_activate_command = format!("{} --boot", self_activate_command);
    }

    if data.test {
        self_activate_command = format!("{} --test", self_activate_command);
    }

    if let Some(sudo_cmd) = &data.sudo {
        self_activate_command = format!("{} {}", sudo_cmd, self_activate_command);
    }

    self_activate_command
}

#[test]
fn test_activation_command_builder() {
    let sudo = Some("sudo -u test".to_string());
    let profile_info = &ProfileInfo::ProfilePath {
        profile_path: "/blah/profiles/test".to_string(),
    };
    let closure = "/nix/store/blah/etc";
    let auto_rollback = true;
    let dry_activate = false;
    let boot = false;
    let test = false;
    let temp_path = Path::new("/tmp");
    let confirm_timeout = 30;
    let magic_rollback = true;
    let debug_logs = true;
    let log_dir = Some("/tmp/something.txt");

    assert_eq!(
        build_activate_command(&ActivateCommandData {
            sudo: &sudo,
            profile_info,
            closure,
            auto_rollback,
            temp_path,
            confirm_timeout,
            magic_rollback,
            debug_logs,
            log_dir,
            dry_activate,
            boot,
            test,
        }),
        "sudo -u test /nix/store/blah/etc/activate-rs --debug-logs --log-dir /tmp/something.txt activate '/nix/store/blah/etc' --profile-path '/blah/profiles/test' --temp-path '/tmp' --confirm-timeout 30 --magic-rollback --auto-rollback"
            .to_string(),
    );
}

struct WaitCommandData<'a> {
    sudo: &'a Option<String>,
    closure: &'a str,
    temp_path: &'a Path,
    activation_timeout: Option<u16>,
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
        self_activate_command,
        data.closure,
        data.temp_path.display(),
    );
    if let Some(activation_timeout) = data.activation_timeout {
        self_activate_command = format!(
            "{} --activation-timeout {}",
            self_activate_command, activation_timeout
        );
    }

    if let Some(sudo_cmd) = &data.sudo {
        self_activate_command = format!("{} {}", sudo_cmd, self_activate_command);
    }

    self_activate_command
}

#[test]
fn test_wait_command_builder() {
    let sudo = Some("sudo -u test".to_string());
    let closure = "/nix/store/blah/etc";
    let temp_path = Path::new("/tmp");
    let activation_timeout = Some(600);
    let debug_logs = true;
    let log_dir = Some("/tmp/something.txt");

    assert_eq!(
        build_wait_command(&WaitCommandData {
            sudo: &sudo,
            closure,
            temp_path,
            activation_timeout,
            debug_logs,
            log_dir
        }),
        "sudo -u test /nix/store/blah/etc/activate-rs --debug-logs --log-dir /tmp/something.txt wait '/nix/store/blah/etc' --temp-path '/tmp' --activation-timeout 600"
            .to_string(),
    );
}

struct RevokeCommandData<'a> {
    sudo: &'a Option<String>,
    closure: &'a str,
    profile_info: ProfileInfo,
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

    self_activate_command = format!(
        "{} revoke {}",
        self_activate_command,
        match &data.profile_info {
            ProfileInfo::ProfilePath { profile_path } =>
                format!("--profile-path '{}'", profile_path),
            ProfileInfo::ProfileUserAndName {
                profile_user,
                profile_name,
            } => format!(
                "--profile-user {} --profile-name {}",
                profile_user, profile_name
            ),
        }
    );

    if let Some(sudo_cmd) = &data.sudo {
        self_activate_command = format!("{} {}", sudo_cmd, self_activate_command);
    }

    self_activate_command
}

#[test]
fn test_revoke_command_builder() {
    let sudo = Some("sudo -u test".to_string());
    let closure = "/nix/store/blah/etc";
    let profile_info = ProfileInfo::ProfilePath {
        profile_path: "/nix/var/nix/per-user/user/profile".to_string(),
    };
    let debug_logs = true;
    let log_dir = Some("/tmp/something.txt");

    assert_eq!(
        build_revoke_command(&RevokeCommandData {
            sudo: &sudo,
            closure,
            profile_info,
            debug_logs,
            log_dir
        }),
        "sudo -u test /nix/store/blah/etc/activate-rs --debug-logs --log-dir /tmp/something.txt revoke --profile-path '/nix/var/nix/per-user/user/profile'"
            .to_string(),
    );
}

async fn handle_sudo_stdin(
    ssh_activate_child: &mut tokio::process::Child,
    deploy_defs: &DeployDefs,
) -> Result<(), std::io::Error> {
    match ssh_activate_child.stdin.as_mut() {
        Some(stdin) => {
            let _ = stdin
                .write_all(
                    format!(
                        "{}\n",
                        deploy_defs.sudo_password.clone().unwrap_or("".to_string())
                    )
                    .as_bytes(),
                )
                .await;
            Ok(())
        }
        None => Err(std::io::Error::other(
            "Failed to open stdin for sudo command",
        )),
    }
}

#[derive(Error, Debug)]
pub enum SSHConfirmError {}

impl command::HasCommandError for SSHConfirmError {
    fn title() -> String {
        "SSH confirmation command (the server should roll back)".to_string()
    }
}

#[derive(Error, Debug)]
pub enum ConfirmProfileError {
    #[error("{0}")]
    SSHConfirm(#[from] command::CommandError<SSHConfirmError>),
}

pub async fn confirm_profile(
    deploy_data: &super::DeployData,
    deploy_defs: &super::DeployDefs,
    temp_path: &Path,
    ssh_addr: &str,
    closure: &str,
) -> Result<(), ConfirmProfileError> {
    let mut ssh_confirm_command = Command::new("ssh");
    ssh_confirm_command
        .arg(ssh_addr)
        .stdin(std::process::Stdio::piped());

    for ssh_opt in &deploy_data.merged_settings.ssh_opts {
        ssh_confirm_command.arg(ssh_opt);
    }

    let lock_path = super::make_lock_path(temp_path, closure);

    let mut confirm_command = format!("rm {}", lock_path.display());
    if let Some(sudo_cmd) = &deploy_defs.sudo {
        confirm_command = format!("{} {}", sudo_cmd, confirm_command);
    }

    debug!(
        "Attempting to run command to confirm deployment: {}",
        confirm_command
    );

    let mut ssh_confirm_child = ssh_confirm_command
        .arg(confirm_command)
        .spawn()
        .map_err(|e| ConfirmProfileError::SSHConfirm(command::CommandError::RunError(e)))?;

    if deploy_data
        .merged_settings
        .interactive_sudo
        .unwrap_or(false)
    {
        trace!("[confirm] Piping in sudo password");
        handle_sudo_stdin(&mut ssh_confirm_child, deploy_defs)
            .await
            .map_err(|err| ConfirmProfileError::SSHConfirm(command::CommandError::RunError(err)))?;
    }

    let ssh_confirm_exit_status = ssh_confirm_child
        .wait()
        .await
        .map_err(|err| ConfirmProfileError::SSHConfirm(command::CommandError::RunError(err)))?;

    match ssh_confirm_exit_status.code() {
        Some(0) => (),
        _exit_code => {
            return Err(ConfirmProfileError::SSHConfirm(
                command::CommandError::ExitStatus(
                    ssh_confirm_exit_status,
                    format!("{:?}", ssh_confirm_command),
                ),
            ))
        }
    };

    info!("Deployment confirmed.");

    Ok(())
}

#[derive(Error, Debug)]
pub enum SSHActivateError {
    #[error("Failed to spawn activation command over SSH: {0}")]
    OtherError(std::io::Error),
    #[error("Failed to pipe to child stdin: {0}")]
    PipeError(std::io::Error),
    #[error("Activating over SSH resulted in a bad exit code: {0:?}")]
    Timeout(tokio::sync::oneshot::error::RecvError),
}

impl command::HasCommandError for SSHActivateError {
    fn title() -> String {
        "SSH activation command".to_string()
    }
}

#[derive(Error, Debug)]
pub enum SSHWaitError {}

impl command::HasCommandError for SSHWaitError {
    fn title() -> String {
        "SSH wait command".to_string()
    }
}

#[derive(Error, Debug)]
pub enum DeployProfileError {
    #[error("{0}")]
    SSHActivate(#[from] command::CommandError<SSHActivateError>),

    #[error("{0}")]
    SSHWait(#[from] command::CommandError<SSHWaitError>),

    #[error("Error confirming deployment: {0}")]
    Confirm(#[from] ConfirmProfileError),
    #[error("Deployment data invalid: {0}")]
    InvalidDeployDataDefs(#[from] DeployDataDefsError),
}

async fn forward_reader<R: tokio::io::AsyncRead + Unpin, W: tokio::io::AsyncWrite + Unpin>(
    mut reader: R,
    mut writer: W,
) -> std::io::Result<()> {
    const BUF_SIZE: usize = 65536;
    let prefix = &'📠'.to_string().into_bytes();
    let mut buf = [0u8; BUF_SIZE];

    // We could instead use None, but having '\n' here is simpler and has the same effect.
    let mut previous_byte = b'\n';
    let mut out = Vec::with_capacity(BUF_SIZE);
    loop {
        let num_bytes_read = reader.read(&mut buf[..]).await?;
        if num_bytes_read == 0 {
            return Ok(());
        }

        out.clear();
        buf[..num_bytes_read].iter().for_each(|current_byte| {
            if previous_byte == b'\n' {
                out.extend_from_slice(prefix);
                out.push(b' ');
            }
            out.push(*current_byte);
            previous_byte = *current_byte;
        });

        if let Err(err) = writer.write_all(&out[..]).await {
            // Keep draining the reader so the child on the other end of the
            // pipe never blocks trying to write more than its OS pipe buffer
            // holds, even though we can no longer show its output.
            while reader.read(&mut buf[..]).await? != 0 {}
            return Err(err);
        }
    }
}

pub async fn deploy_profile(
    deploy_data: &super::DeployData,
    deploy_defs: &super::DeployDefs,
    closure: &str,
    dry_activate: bool,
    boot: bool,
    test: bool,
) -> Result<(), DeployProfileError> {
    if !dry_activate {
        info!(
            "Activating profile `{}` for node `{}`",
            deploy_data.profile_name, deploy_data.node_name
        );
    }

    let temp_path: &Path = match &deploy_data.merged_settings.temp_path {
        Some(x) => x,
        None => Path::new("/tmp"),
    };

    let confirm_timeout = deploy_data.merged_settings.confirm_timeout.unwrap_or(30);

    let activation_timeout = deploy_data.merged_settings.activation_timeout;

    let magic_rollback = deploy_data.merged_settings.magic_rollback.unwrap_or(true);

    let auto_rollback = deploy_data.merged_settings.auto_rollback.unwrap_or(true);

    let self_activate_command = build_activate_command(&ActivateCommandData {
        sudo: &deploy_defs.sudo,
        profile_info: &deploy_data.get_profile_info()?,
        closure,
        auto_rollback,
        temp_path,
        confirm_timeout,
        magic_rollback,
        debug_logs: deploy_data.debug_logs,
        log_dir: deploy_data.log_dir.as_deref(),
        dry_activate,
        boot,
        test,
    });

    debug!("Constructed activation command: {}", self_activate_command);

    let hostname = match deploy_data.cmd_overrides.hostname {
        Some(ref x) => x,
        None => &deploy_data.node.node_settings.hostname,
    };

    let ssh_addr = format!("{}@{}", deploy_defs.ssh_user, hostname);

    let mut ssh_activate_command = Command::new("ssh");
    ssh_activate_command
        .arg(&ssh_addr)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .stdin(std::process::Stdio::piped());

    for ssh_opt in &deploy_data.merged_settings.ssh_opts {
        ssh_activate_command.arg(ssh_opt);
    }

    fn warn_stdstream_task_err(
        stream_name: &str,
        stdstream_task_result: Result<(), std::io::Error>,
    ) {
        if let Err(err) = stdstream_task_result {
            warn!("Error while decoding {}: {}", stream_name, err)
        }
    }

    if !magic_rollback || dry_activate || boot {
        let mut ssh_activate_child = ssh_activate_command
            .arg(self_activate_command)
            .spawn()
            .map_err(|err| {
                DeployProfileError::SSHActivate(command::CommandError::OtherError(
                    SSHActivateError::OtherError(err),
                ))
            })?;

        let stdout = ssh_activate_child.stdout.take().expect("stdout piped");
        let stderr = ssh_activate_child.stderr.take().expect("stderr piped");

        let stdout_task = forward_reader(stdout, tokio::io::stdout());
        let stderr_task = forward_reader(stderr, tokio::io::stderr());

        if deploy_data
            .merged_settings
            .interactive_sudo
            .unwrap_or(false)
        {
            trace!("[activate] Piping in sudo password");
            handle_sudo_stdin(&mut ssh_activate_child, deploy_defs)
                .await
                .map_err(|err| {
                    DeployProfileError::SSHActivate(command::CommandError::OtherError(
                        SSHActivateError::PipeError(err),
                    ))
                })?;
        }

        let (ssh_activate_result, stdout_task_result, stderr_task_result) =
            tokio::join!(ssh_activate_child.wait(), stdout_task, stderr_task);

        match ssh_activate_result {
            Err(err) => {
                return Err(DeployProfileError::SSHActivate(
                    command::CommandError::RunError(err),
                ))
            }
            Ok(ssh_activate_exit_status) => {
                match ssh_activate_exit_status.code() {
                    Some(0) => (),
                    _exit_code => {
                        return Err(DeployProfileError::SSHActivate(
                            command::CommandError::ExitStatus(
                                ssh_activate_exit_status,
                                format!("{:?}", ssh_activate_command),
                            ),
                        ))
                    }
                };
            }
        }

        warn_stdstream_task_err("stdout", stdout_task_result);
        warn_stdstream_task_err("stderr", stderr_task_result);

        if dry_activate {
            info!("Completed dry-activate!");
        } else if boot {
            info!("Success activating for next boot, done!");
        } else {
            info!("Success activating, done!");
        }
    } else {
        let self_wait_command = build_wait_command(&WaitCommandData {
            sudo: &deploy_defs.sudo,
            closure,
            temp_path,
            activation_timeout,
            debug_logs: deploy_data.debug_logs,
            log_dir: deploy_data.log_dir.as_deref(),
        });

        debug!("Constructed wait command: {}", self_wait_command);

        let mut ssh_activate_child = ssh_activate_command
            .arg(self_activate_command)
            .spawn()
            .map_err(|err| {
                DeployProfileError::SSHActivate(command::CommandError::OtherError(
                    SSHActivateError::OtherError(err),
                ))
            })?;

        let stdout = ssh_activate_child.stdout.take().expect("stdout piped");
        let stderr = ssh_activate_child.stderr.take().expect("stderr piped");

        let stdout_task = forward_reader(stdout, tokio::io::stdout());
        let stderr_task = forward_reader(stderr, tokio::io::stderr());

        if deploy_data
            .merged_settings
            .interactive_sudo
            .unwrap_or(false)
        {
            trace!("[activate] Piping in sudo password");
            handle_sudo_stdin(&mut ssh_activate_child, deploy_defs)
                .await
                .map_err(|err| {
                    DeployProfileError::SSHActivate(command::CommandError::OtherError(
                        SSHActivateError::PipeError(err),
                    ))
                })?;
        }

        info!("Creating activation waiter");

        let mut ssh_wait_command = Command::new("ssh");
        ssh_wait_command
            .arg(&ssh_addr)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .stdin(std::process::Stdio::piped());

        for ssh_opt in &deploy_data.merged_settings.ssh_opts {
            ssh_wait_command.arg(ssh_opt);
        }

        let (send_activate, recv_activate) = tokio::sync::oneshot::channel();
        let (send_activated, recv_activated) = tokio::sync::oneshot::channel();

        let thread = tokio::spawn(async move {
            let (ssh_activate_result, stdout_task_result, stderr_task_result) =
                tokio::join!(ssh_activate_child.wait(), stdout_task, stderr_task);

            let maybe_err = match ssh_activate_result {
                Err(err) => Some(DeployProfileError::SSHActivate(
                    command::CommandError::RunError(err),
                )),
                Ok(ref exit_status) => match exit_status.code() {
                    Some(0) => None,
                    _exit_code => Some(DeployProfileError::SSHActivate(
                        command::CommandError::ExitStatus(
                            *exit_status,
                            format!("{:?}", ssh_activate_command),
                        ),
                    )),
                },
            };

            warn_stdstream_task_err("stdout", stdout_task_result);
            warn_stdstream_task_err("stderr", stderr_task_result);

            if let Some(err) = maybe_err {
                // The receiver is gone if the main task already returned; that is fine.
                let _ = send_activate.send(err);
            }

            let _ = send_activated.send(());
        });

        let mut ssh_wait_child = ssh_wait_command
            .arg(self_wait_command)
            .spawn()
            .map_err(|err| DeployProfileError::SSHWait(command::CommandError::RunError(err)))?;

        let wait_stdout = ssh_wait_child.stdout.take().expect("stdout piped");
        let wait_stderr = ssh_wait_child.stderr.take().expect("stderr piped");

        let wait_stdout_task = forward_reader(wait_stdout, tokio::io::stdout());
        let wait_stderr_task = forward_reader(wait_stderr, tokio::io::stderr());

        if deploy_data
            .merged_settings
            .interactive_sudo
            .unwrap_or(false)
        {
            trace!("[wait] Piping in sudo password");
            handle_sudo_stdin(&mut ssh_wait_child, deploy_defs)
                .await
                .map_err(|err| {
                    DeployProfileError::SSHActivate(command::CommandError::OtherError(
                        SSHActivateError::PipeError(err),
                    ))
                })?;
        }

        tokio::select! {
            x = async {
                let (result, stdout_task_result, stderr_task_result) =
                    tokio::join!(ssh_wait_child.wait(), wait_stdout_task, wait_stderr_task);
                warn_stdstream_task_err("stdout", stdout_task_result);
                warn_stdstream_task_err("stderr", stderr_task_result);
                result
            } => {
                debug!("Wait command ended");
                let status = x.map_err(|err| DeployProfileError::SSHWait(command::CommandError::RunError(err)))?;
                match status.code() {
                    Some(0) => (),
                    _exit_code => return Err(DeployProfileError::SSHWait(
                        command::CommandError::ExitStatus(status, format!("{:?}", ssh_wait_command))
                    )),
                };
            },
            x = recv_activate => {
                match x {
                    Ok(err) => {
                        debug!("Activate command exited with an error");
                        return Err(err);
                    }
                    // The sender was dropped without sending an error, which means
                    // activation finished successfully before the wait command returned.
                    Err(_) => debug!("Activation finished before the wait command returned"),
                }
            },
        }

        info!("Success activating, attempting to confirm activation");

        let c = confirm_profile(deploy_data, deploy_defs, temp_path, &ssh_addr, closure).await;
        recv_activated.await.map_err(|x| {
            DeployProfileError::SSHActivate(command::CommandError::OtherError(
                SSHActivateError::Timeout(x),
            ))
        })?;
        c?;

        thread.await.map_err(|x| {
            DeployProfileError::SSHActivate(command::CommandError::RunError(x.into()))
        })?;
    }

    Ok(())
}

#[derive(Error, Debug)]
pub enum SSHRevokeError {
    #[error("Failed to spawn revocation command over SSH: {0}")]
    SpawnRevokeError(std::io::Error),
}

impl command::HasCommandError for SSHRevokeError {
    fn title() -> String {
        "SSH revoke command".to_string()
    }
}

#[derive(Error, Debug)]
pub enum RevokeProfileError {
    #[error("{0}")]
    SSHRevoke(#[from] command::CommandError<SSHRevokeError>),

    #[error("Deployment data invalid: {0}")]
    InvalidDeployDataDefs(#[from] DeployDataDefsError),
}
pub async fn revoke(
    deploy_data: &crate::DeployData,
    deploy_defs: &crate::DeployDefs,
    closure: &str,
) -> Result<(), RevokeProfileError> {
    let self_revoke_command = build_revoke_command(&RevokeCommandData {
        sudo: &deploy_defs.sudo,
        closure,
        profile_info: deploy_data.get_profile_info()?,
        debug_logs: deploy_data.debug_logs,
        log_dir: deploy_data.log_dir.as_deref(),
    });

    debug!("Constructed revoke command: {}", self_revoke_command);

    let hostname = match deploy_data.cmd_overrides.hostname {
        Some(ref x) => x,
        None => &deploy_data.node.node_settings.hostname,
    };

    let ssh_addr = format!("{}@{}", deploy_defs.ssh_user, hostname);

    let mut ssh_activate_command = Command::new("ssh");
    ssh_activate_command
        .arg(&ssh_addr)
        .stdin(std::process::Stdio::piped());

    for ssh_opt in &deploy_data.merged_settings.ssh_opts {
        ssh_activate_command.arg(ssh_opt);
    }

    let mut ssh_revoke_child = ssh_activate_command
        .arg(self_revoke_command)
        .spawn()
        .map_err(|err| {
            RevokeProfileError::SSHRevoke(command::CommandError::OtherError(
                SSHRevokeError::SpawnRevokeError(err),
            ))
        })?;

    if deploy_data
        .merged_settings
        .interactive_sudo
        .unwrap_or(false)
    {
        trace!("[revoke] Piping in sudo password");
        handle_sudo_stdin(&mut ssh_revoke_child, deploy_defs)
            .await
            .map_err(|err| RevokeProfileError::SSHRevoke(command::CommandError::RunError(err)))?;
    }

    let result = ssh_revoke_child.wait_with_output().await;

    match result {
        Err(x) => Err(RevokeProfileError::SSHRevoke(
            command::CommandError::RunError(x),
        )),
        Ok(ref x) => match x.status.code() {
            Some(0) => Ok(()),
            _exit_code => Err(RevokeProfileError::SSHRevoke(command::CommandError::Exit(
                x.clone(),
                format!("{:?}", ssh_activate_command),
            ))),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_forward_reader_prefixes_each_line() {
        let mut out = Vec::new();
        forward_reader(&b"hello\nworld\n"[..], &mut out)
            .await
            .unwrap();
        assert_eq!(String::from_utf8(out).unwrap(), "📠 hello\n📠 world\n");
    }

    #[tokio::test]
    async fn test_forward_reader_prefixes_last_line_without_trailing_newline() {
        let mut out = Vec::new();
        forward_reader(&b"hello\nworld"[..], &mut out)
            .await
            .unwrap();
        assert_eq!(String::from_utf8(out).unwrap(), "📠 hello\n📠 world");
    }

    #[tokio::test]
    async fn test_forward_reader_empty_input() {
        let mut out = Vec::new();
        forward_reader(&b""[..], &mut out).await.unwrap();
        assert!(out.is_empty());
    }

    #[tokio::test]
    async fn test_forward_reader_prefixes_blank_lines_between_content() {
        let mut out = Vec::new();
        forward_reader(&b"a\n\n\nb\n"[..], &mut out).await.unwrap();
        assert_eq!(String::from_utf8(out).unwrap(), "📠 a\n📠 \n📠 \n📠 b\n");
    }

    #[tokio::test]
    async fn test_forward_reader_prefixes_leading_newline() {
        let mut out = Vec::new();
        forward_reader(&b"\nfoo\n"[..], &mut out).await.unwrap();
        assert_eq!(String::from_utf8(out).unwrap(), "📠 \n📠 foo\n");
    }

    #[tokio::test]
    async fn test_forward_reader_long_line_split_across_many_reads() {
        let (mut writer, reader) = tokio::io::duplex(4);
        let input: Vec<u8> = (0..500).map(|_| b'x').collect();
        let expected_body = String::from_utf8(input.clone()).unwrap();

        let write_task = tokio::spawn(async move {
            writer.write_all(&input).await.unwrap();
        });

        let mut out = Vec::new();
        forward_reader(reader, &mut out).await.unwrap();
        write_task.await.unwrap();

        assert_eq!(
            String::from_utf8(out).unwrap(),
            format!("📠 {}", expected_body)
        );
    }

    #[tokio::test]
    async fn test_forward_reader_newline_split_across_reads() {
        let (mut writer, reader) = tokio::io::duplex(1);
        let input = b"abc\ndef\n".to_vec();

        let write_task = tokio::spawn(async move {
            writer.write_all(&input).await.unwrap();
        });

        let mut out = Vec::new();
        forward_reader(reader, &mut out).await.unwrap();
        write_task.await.unwrap();

        assert_eq!(String::from_utf8(out).unwrap(), "📠 abc\n📠 def\n");
    }
}
