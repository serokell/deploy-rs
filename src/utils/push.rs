// SPDX-FileCopyrightText: 2020 Serokell <https://serokell.io/>
//
// SPDX-License-Identifier: MPL-2.0

use std::process::Stdio;
use tokio::process::Command;

pub async fn push_profile(
    supports_flakes: bool,
    check_sigs: bool,
    repo: &str,
    deploy_data: &super::DeployData<'_>,
    deploy_defs: &super::DeployDefs<'_>,
) -> Result<(), Box<dyn std::error::Error>> {
    info!(
        "Pushing profile `{}` for node `{}`",
        deploy_data.profile_name, deploy_data.node_name
    );

    debug!(
        "Building profile `{} for node `{}`",
        deploy_data.profile_name, deploy_data.node_name
    );

    if supports_flakes {
        Command::new("nix")
            .arg("build")
            .arg("--no-link")
            .arg(format!(
                "{}#deploy.nodes.{}.profiles.{}.path",
                repo, deploy_data.node_name, deploy_data.profile_name
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
                deploy_data.node_name, deploy_data.profile_name
            ))
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()?
            .await?;
    }

    if let Ok(local_key) = std::env::var("LOCAL_KEY") {
        info!(
            "Signing key present! Signing profile `{}` for node `{}`",
            deploy_data.profile_name, deploy_data.node_name
        );

        Command::new("nix")
            .arg("sign-paths")
            .arg("-r")
            .arg("-k")
            .arg(local_key)
            .arg(&deploy_data.profile.profile_settings.path)
            .arg(&super::deploy_path_to_activate_path_str(
                &deploy_defs.current_exe,
            )?)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()?
            .await?;
    }

    debug!(
        "Copying profile `{} for node `{}`",
        deploy_data.profile_name, deploy_data.node_name
    );

    let mut copy_command_ = Command::new("nix");
    let mut copy_command = copy_command_.arg("copy");

    if deploy_data.merged_settings.fast_connection {
        copy_command = copy_command.arg("--substitute-on-destination");
    }

    if !check_sigs {
        copy_command = copy_command.arg("--no-check-sigs");
    }

    let ssh_opts_str = deploy_data
        .merged_settings
        .ssh_opts
        // This should provide some extra safety, but it also breaks for some reason, oh well
        // .iter()
        // .map(|x| format!("'{}'", x))
        // .collect::<Vec<String>>()
        .join(" ");

    let hostname = match deploy_data.cmd_overrides.hostname {
        Some(ref x) => x,
        None => &deploy_data.node.node_settings.hostname,
    };

    copy_command
        .arg("--to")
        .arg(format!("ssh://{}@{}", deploy_defs.ssh_user, hostname))
        .arg(&deploy_data.profile.profile_settings.path)
        .arg(&super::deploy_path_to_activate_path_str(
            &deploy_defs.current_exe,
        )?)
        .env("NIX_SSHOPTS", ssh_opts_str)
        .spawn()?
        .await?;

    Ok(())
}
