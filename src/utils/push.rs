// SPDX-FileCopyrightText: 2020 Serokell <https://serokell.io/>
//
// SPDX-License-Identifier: MPL-2.0

use std::process::Stdio;
use tokio::process::Command;

use thiserror::Error;

#[derive(Error, Debug)]
pub enum PushProfileError {
    #[error("Failed to calculate activate bin path from deploy bin path: {0}")]
    DeployPathToActivatePathError(#[from] super::DeployPathToActivatePathError),
    #[error("Failed to run Nix build command: {0}")]
    BuildError(std::io::Error),
    #[error("Nix build command resulted in a bad exit code: {0:?}")]
    BuildExitError(Option<i32>),
    #[error("Failed to run Nix sign command: {0}")]
    SignError(std::io::Error),
    #[error("Nix sign command resulted in a bad exit code: {0:?}")]
    SignExitError(Option<i32>),
    #[error("Failed to run Nix copy command: {0}")]
    CopyError(std::io::Error),
    #[error("Nix copy command resulted in a bad exit code: {0:?}")]
    CopyExitError(Option<i32>),
}

pub async fn push_profile(
    supports_flakes: bool,
    check_sigs: bool,
    repo: &str,
    deploy_data: &super::DeployData<'_>,
    deploy_defs: &super::DeployDefs<'_>,
    keep_result: bool,
    result_path: Option<&str>,
) -> Result<(), PushProfileError> {
    info!(
        "Building profile `{}` for node `{}`",
        deploy_data.profile_name, deploy_data.node_name
    );

    let mut build_c = if supports_flakes {
        Command::new("nix")
    } else {
        Command::new("nix-build")
    };

    let mut build_command = if supports_flakes {
        build_c.arg("build").arg("--no-link").arg(format!(
            "{}#deploy.nodes.{}.profiles.{}.path",
            repo, deploy_data.node_name, deploy_data.profile_name
        ))
    } else {
        build_c
            .arg(&repo)
            .arg("--no-out-link")
            .arg("-A")
            .arg(format!(
                "deploy.nodes.{}.profiles.{}.path",
                deploy_data.node_name, deploy_data.profile_name
            ))
    };

    build_command = match (keep_result, supports_flakes) {
        (true, _) => {
            let result_path = match result_path {
                Some(x) => x,
                None => "./.deploy-gc",
            };

            build_command.arg("--out-link").arg(format!(
                "{}/{}/{}",
                result_path, deploy_data.node_name, deploy_data.profile_name
            ))
        }
        (false, false) => build_command.arg("--no-out-link"),
        (false, true) => build_command.arg("--no-link"),
    };

    let build_exit_status = build_command
        .stdout(Stdio::null())
        .status()
        .await
        .map_err(PushProfileError::BuildError)?;

    match build_exit_status.code() {
        Some(0) => (),
        a => return Err(PushProfileError::BuildExitError(a)),
    };

    if let Ok(local_key) = std::env::var("LOCAL_KEY") {
        info!(
            "Signing key present! Signing profile `{}` for node `{}`",
            deploy_data.profile_name, deploy_data.node_name
        );

        let sign_exit_status = Command::new("nix")
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
            .status()
            .await
            .map_err(PushProfileError::SignError)?;

        match sign_exit_status.code() {
            Some(0) => (),
            a => return Err(PushProfileError::SignExitError(a)),
        };
    }

    debug!(
        "Copying profile `{}` to node `{}`",
        deploy_data.profile_name, deploy_data.node_name
    );

    let mut copy_command_ = Command::new("nix");
    let mut copy_command = copy_command_.arg("copy");

    if let Some(true) = deploy_data.merged_settings.fast_connection {
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

    let copy_exit_status = copy_command
        .arg("--to")
        .arg(format!("ssh://{}@{}", deploy_defs.ssh_user, hostname))
        .arg(&deploy_data.profile.profile_settings.path)
        .arg(&super::deploy_path_to_activate_path_str(
            &deploy_defs.current_exe,
        )?)
        .env("NIX_SSHOPTS", ssh_opts_str)
        .status()
        .await
        .map_err(PushProfileError::CopyError)?;

    match copy_exit_status.code() {
        Some(0) => (),
        a => return Err(PushProfileError::CopyExitError(a)),
    };

    Ok(())
}
