// SPDX-FileCopyrightText: 2020 Serokell <https://serokell.io/>
//
// SPDX-License-Identifier: MPL-2.0

use super::data;

use tokio::process::Command;

fn deploy_path_to_activate_path_str(
    deploy_path: &std::path::Path,
) -> Result<String, Box<dyn std::error::Error>> {
    Ok(format!(
        "{}/activate",
        deploy_path
            .parent()
            .ok_or("Deploy path too short")?
            .to_str()
            .ok_or("Deploy path is not valid utf8")?
            .to_owned()
    ))
}

#[test]
fn test_activate_path_generation() {
    match deploy_path_to_activate_path_str(&std::path::PathBuf::from(
        "/blah/blah/deploy-rs/bin/deploy",
    )) {
        Err(_) => panic!(""),
        Ok(x) => assert_eq!(x, "/blah/blah/deploy-rs/bin/activate".to_string()),
    }
}

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
    profile: &data::Profile,
    profile_name: &str,
    node: &data::Node,
    node_name: &str,
    merged_settings: &data::GenericSettings,
    deploy_data: &super::DeployData<'_>,
    auto_rollback: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    info!(
        "Activating profile `{}` for node `{}`",
        profile_name, node_name
    );

    let activate_path_str = deploy_path_to_activate_path_str(&deploy_data.current_exe)?;

    let self_activate_command = build_activate_command(
        activate_path_str,
        &deploy_data.sudo,
        &deploy_data.profile_path,
        &profile.profile_settings.path,
        &profile.profile_settings.activate,
        &profile.profile_settings.bootstrap,
        auto_rollback,
    )?;

    let mut c = Command::new("ssh");
    let mut ssh_command = c.arg(format!(
        "ssh://{}@{}",
        deploy_data.ssh_user, node.node_settings.hostname
    ));

    for ssh_opt in &merged_settings.ssh_opts {
        ssh_command = ssh_command.arg(ssh_opt);
    }

    ssh_command.arg(self_activate_command).spawn()?.await?;

    Ok(())
}
