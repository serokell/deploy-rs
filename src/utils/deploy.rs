use super::data;

use tokio::process::Command;

pub async fn deploy_profile(
    profile: &data::Profile,
    profile_name: &str,
    node: &data::Node,
    node_name: &str,
    merged_settings: &data::GenericSettings,
    deploy_data: &super::DeployData<'_>,
) -> Result<(), Box<dyn std::error::Error>> {
    info!(
        "Activating profile `{}` for node `{}`",
        profile_name, node_name
    );

    let mut self_activate_command = format!(
        "{} activate '{}' '{}'",
        deploy_data.current_exe.as_path().to_str().unwrap(),
        deploy_data.profile_path,
        profile.profile_settings.path,
    );

    if let Some(sudo_cmd) = &deploy_data.sudo {
        self_activate_command = format!("{} {}", sudo_cmd, self_activate_command);
    }

    if let Some(ref bootstrap_cmd) = profile.profile_settings.bootstrap {
        self_activate_command = format!(
            "{} --bootstrap-cmd '{}'",
            self_activate_command, bootstrap_cmd
        );
    }

    if let Some(ref activate_cmd) = profile.profile_settings.activate {
        self_activate_command = format!(
            "{} --activate-cmd '{}'",
            self_activate_command, activate_cmd
        );
    }

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
