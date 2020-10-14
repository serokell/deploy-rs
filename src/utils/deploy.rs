// SPDX-FileCopyrightText: 2020 Serokell <https://serokell.io/>
//
// SPDX-License-Identifier: MPL-2.0

use std::borrow::Cow;
use tokio::process::Command;

fn build_activate_command(
    activate_path_str: String,
    sudo: &Option<String>,
    profile_path: &str,
    closure: &str,
    bootstrap_cmd: &Option<String>,
    auto_rollback: bool,
    temp_path: &Cow<str>,
    confirm_timeout: u16,
    magic_rollback: bool,
) -> String {
    let mut self_activate_command = format!(
        "{} '{}' '{}' --temp-path {} --confirm-timeout {}",
        activate_path_str, profile_path, closure, temp_path, confirm_timeout
    );

    if magic_rollback {
        self_activate_command = format!("{} --magic-rollback", self_activate_command);
    }

    if auto_rollback {
        self_activate_command = format!("{} --auto-rollback", self_activate_command);
    }

    if let Some(ref bootstrap_cmd) = bootstrap_cmd {
        self_activate_command = format!(
            "{} --bootstrap-cmd '{}'",
            self_activate_command, bootstrap_cmd
        );
    }

    if let Some(sudo_cmd) = &sudo {
        self_activate_command = format!("{} {}", sudo_cmd, self_activate_command);
    }

    self_activate_command
}

#[test]
fn test_activation_command_builder() {
    let activate_path_str = "/blah/bin/activate".to_string();
    let sudo = Some("sudo -u test".to_string());
    let profile_path = "/blah/profiles/test";
    let closure = "/blah/etc";
    let bootstrap_cmd = None;
    let auto_rollback = true;
    let temp_path = &"/tmp/deploy-rs".into();
    let confirm_timeout = 30;
    let magic_rollback = true;

    assert_eq!(
        build_activate_command(
            activate_path_str,
            &sudo,
            profile_path,
            closure,
            &bootstrap_cmd,
            auto_rollback,
            temp_path,
            confirm_timeout,
            magic_rollback
        ),
        "sudo -u test /blah/bin/activate '/blah/profiles/test' '/blah/etc' --temp-path /tmp/deploy-rs --confirm-timeout 30 --magic-rollback --auto-rollback"
            .to_string(),
    );
}

pub async fn deploy_profile(
    deploy_data: &super::DeployData<'_>,
    deploy_defs: &super::DeployDefs<'_>,
) -> Result<(), Box<dyn std::error::Error>> {
    info!(
        "Activating profile `{}` for node `{}`",
        deploy_data.profile_name, deploy_data.node_name
    );

    let activate_path_str = super::deploy_path_to_activate_path_str(&deploy_defs.current_exe)?;

    let temp_path: Cow<str> = match &deploy_data.merged_settings.temp_path {
        Some(x) => x.into(),
        None => "/tmp/deploy-rs".into(),
    };

    let confirm_timeout = deploy_data.merged_settings.confirm_timeout.unwrap_or(30);

    let magic_rollback = deploy_data.merged_settings.magic_rollback.unwrap_or(false);

    let auto_rollback = deploy_data.merged_settings.auto_rollback.unwrap_or(true);

    let self_activate_command = build_activate_command(
        activate_path_str,
        &deploy_defs.sudo,
        &deploy_defs.profile_path,
        &deploy_data.profile.profile_settings.path,
        &deploy_data.profile.profile_settings.bootstrap,
        auto_rollback,
        &temp_path,
        confirm_timeout,
        magic_rollback,
    );

    let hostname = match deploy_data.cmd_overrides.hostname {
        Some(ref x) => x,
        None => &deploy_data.node.node_settings.hostname,
    };

    let mut c = Command::new("ssh");
    let mut ssh_command = c.arg(format!("ssh://{}@{}", deploy_defs.ssh_user, hostname));

    for ssh_opt in &deploy_data.merged_settings.ssh_opts {
        ssh_command = ssh_command.arg(ssh_opt);
    }

    let ssh_exit_status = ssh_command.arg(self_activate_command).status().await?;

    if !ssh_exit_status.success() {
        good_panic!("Activation over SSH failed");
    }

    info!("Success activating!");

    if magic_rollback {
        info!("Attempting to confirm activation");

        let mut c = Command::new("ssh");
        let mut ssh_confirm_command = c.arg(format!("ssh://{}@{}", deploy_defs.ssh_user, hostname));

        for ssh_opt in &deploy_data.merged_settings.ssh_opts {
            ssh_confirm_command = ssh_confirm_command.arg(ssh_opt);
        }

        let lock_hash = &deploy_data.profile.profile_settings.path[11 /* /nix/store/ */ ..];
        let lock_path = format!("{}/activating-{}", temp_path, lock_hash);

        let mut confirm_command = format!("rm {}", lock_path);
        if let Some(sudo_cmd) = &deploy_defs.sudo {
            confirm_command = format!("{} {}", sudo_cmd, confirm_command);
        }

        let ssh_exit_status = ssh_confirm_command.arg(confirm_command).status().await?;

        if !ssh_exit_status.success() {
            good_panic!(
                "Failed to confirm deployment, the node will roll back in <{} seconds",
                confirm_timeout
            );
        }

        info!("Deployment confirmed.");
    }

    Ok(())
}
