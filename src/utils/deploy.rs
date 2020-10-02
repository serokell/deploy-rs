// SPDX-FileCopyrightText: 2020 Serokell <https://serokell.io/>
//
// SPDX-License-Identifier: MPL-2.0

use tokio::process::Command;

fn build_activate_command(
    activate_path_str: String,
    sudo: &Option<String>,
    profile_path: &str,
    closure: &str,
    activate_cmd: &Option<String>,
    bootstrap_cmd: &Option<String>,
    auto_rollback: bool,
) -> Result<String, Box<dyn std::error::Error>> {
    let mut self_activate_command =
        format!("{} '{}' '{}'", activate_path_str, profile_path, closure);

    if let Some(sudo_cmd) = &sudo {
        self_activate_command = format!("{} {}", sudo_cmd, self_activate_command);
    }

    if let Some(ref bootstrap_cmd) = bootstrap_cmd {
        self_activate_command = format!(
            "{} --bootstrap-cmd '{}'",
            self_activate_command, bootstrap_cmd
        );
    }

    if let Some(ref activate_cmd) = activate_cmd {
        self_activate_command = format!(
            "{} --activate-cmd '{}'",
            self_activate_command, activate_cmd
        );
    }

    if auto_rollback {
        self_activate_command = format!("{} --auto-rollback", self_activate_command);
    }

    Ok(self_activate_command)
}

#[test]
fn test_activation_command_builder() {
    let activate_path_str = "/blah/bin/activate".to_string();
    let sudo = Some("sudo -u test".to_string());
    let profile_path = "/blah/profiles/test";
    let closure = "/blah/etc";
    let activate_cmd = Some("$THING/bin/aaaaaaa".to_string());
    let bootstrap_cmd = None;
    let auto_rollback = true;

    match build_activate_command(
        activate_path_str,
        &sudo,
        profile_path,
        closure,
        &activate_cmd,
        &bootstrap_cmd,
        auto_rollback,
    ) {
        Err(_) => panic!(""),
        Ok(x) => assert_eq!(x, "sudo -u test /blah/bin/activate '/blah/profiles/test' '/blah/etc' --activate-cmd '$THING/bin/aaaaaaa' --auto-rollback".to_string()),
    }
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

    let self_activate_command = build_activate_command(
        activate_path_str,
        &deploy_defs.sudo,
        &deploy_defs.profile_path,
        &deploy_data.profile.profile_settings.path,
        &deploy_data.profile.profile_settings.activate,
        &deploy_data.profile.profile_settings.bootstrap,
        deploy_data.merged_settings.auto_rollback,
    )?;

    let hostname = match deploy_data.cmd_overrides.hostname {
        Some(ref x) => x,
        None => &deploy_data.node.node_settings.hostname,
    };

    let mut c = Command::new("ssh");
    let mut ssh_command = c.arg(format!("ssh://{}@{}", deploy_defs.ssh_user, hostname));

    for ssh_opt in &deploy_data.merged_settings.ssh_opts {
        ssh_command = ssh_command.arg(ssh_opt);
    }

    ssh_command.arg(self_activate_command).spawn()?.await?;

    Ok(())
}
