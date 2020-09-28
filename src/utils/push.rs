use super::data;

use std::process::Stdio;
use tokio::process::Command;

pub async fn push_profile(
    profile: &data::Profile,
    profile_name: &str,
    node: &data::Node,
    node_name: &str,
    supports_flakes: bool,
    check_sigs: bool,
    repo: &str,
    merged_settings: &data::GenericSettings,
    deploy_data: &super::DeployData<'_>,
) -> Result<(), Box<dyn std::error::Error>> {
    info!(
        "Pushing profile `{}` for node `{}`",
        profile_name, node_name
    );

    debug!(
        "Building profile `{} for node `{}`",
        profile_name, node_name
    );

    if supports_flakes {
        Command::new("nix")
            .arg("build")
            .arg("--no-link")
            .arg(format!(
                "{}#deploy.nodes.{}.profiles.{}.path",
                repo, node_name, profile_name
            ))
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()?
            .await?;
    } else {
        Command::new("nix-build")
            .arg(&repo)
            .arg("-A")
            .arg(format!(
                "deploy.nodes.{}.profiles.{}.path",
                node_name, profile_name
            ))
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()?
            .await?;
    }

    if let Ok(local_key) = std::env::var("LOCAL_KEY") {
        info!(
            "Signing key present! Signing profile `{}` for node `{}`",
            profile_name, node_name
        );

        Command::new("nix")
            .arg("sign-paths")
            .arg("-r")
            .arg("-k")
            .arg(local_key)
            .arg(&profile.profile_settings.path)
            .arg(&deploy_data.current_exe)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()?
            .await?;
    }

    debug!("Copying profile `{} for node `{}`", profile_name, node_name);

    let mut copy_command_ = Command::new("nix");
    let mut copy_command = copy_command_.arg("copy");

    if merged_settings.fast_connection {
        copy_command = copy_command.arg("--substitute-on-destination");
    }

    if !check_sigs {
        copy_command = copy_command.arg("--no-check-sigs");
    }

    let ssh_opts_str = merged_settings
        .ssh_opts
        // This should provide some extra safety, but it also breaks for some reason, oh well
        // .iter()
        // .map(|x| format!("'{}'", x))
        // .collect::<Vec<String>>()
        .join(" ");

    copy_command
        .arg("--to")
        .arg(format!(
            "ssh://{}@{}",
            deploy_data.ssh_user, node.node_settings.hostname
        ))
        .arg(&profile.profile_settings.path)
        .arg(&deploy_data.current_exe)
        .env("NIX_SSHOPTS", ssh_opts_str)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?
        .await?;

    Ok(())
}
