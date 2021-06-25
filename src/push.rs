// SPDX-FileCopyrightText: 2020 Serokell <https://serokell.io/>
//
// SPDX-License-Identifier: MPL-2.0

use log::{debug, info};
use std::collections::HashMap;
use std::path::Path;
use std::process::Stdio;
use thiserror::Error;
use tokio::process::Command;

#[derive(Error, Debug)]
pub enum PushProfileError {
    #[error("Failed to run Nix show-derivation command: {0}")]
    ShowDerivationError(std::io::Error),
    #[error("Nix show-derivation command resulted in a bad exit code: {0:?}")]
    ShowDerivationExitError(Option<i32>),
    #[error("Nix show-derivation command output contained an invalid UTF-8 sequence: {0}")]
    ShowDerivationUtf8Error(std::str::Utf8Error),
    #[error("Failed to parse the output of nix show-derivation: {0}")]
    ShowDerivationParseError(serde_json::Error),
    #[error("Nix show-derivation output is empty")]
    ShowDerivationEmpty,
    #[error("Failed to run Nix build command: {0}")]
    BuildError(std::io::Error),
    #[error("Nix build command resulted in a bad exit code: {0:?}")]
    BuildExitError(Option<i32>),
    #[error(
        "Activation script deploy-rs-activate does not exist in profile.\n\
             Did you forget to use deploy-rs#lib.<...>.activate.<...> on your profile path?"
    )]
    DeployRsActivateDoesntExist,
    #[error("Activation script activate-rs does not exist in profile.\n\
             Is there a mismatch in deploy-rs used in the flake you're deploying and deploy-rs command you're running?")]
    ActivateRsDoesntExist,
    #[error("Failed to run Nix sign command: {0}")]
    SignError(std::io::Error),
    #[error("Nix sign command resulted in a bad exit code: {0:?}")]
    SignExitError(Option<i32>),
    #[error("Failed to run Nix copy command: {0}")]
    CopyError(std::io::Error),
    #[error("Nix copy command resulted in a bad exit code: {0:?}")]
    CopyExitError(Option<i32>),
}

pub struct PushProfileData<'a> {
    pub supports_flakes: bool,
    pub check_sigs: bool,
    pub repo: &'a str,
    pub deploy_data: &'a super::DeployData<'a>,
    pub deploy_defs: &'a super::DeployDefs,
    pub keep_result: bool,
    pub result_path: Option<&'a str>,
    pub extra_build_args: &'a [String],
}

pub async fn push_profile(data: PushProfileData<'_>) -> Result<(), PushProfileError> {
    debug!(
        "Finding the deriver of store path for {}",
        &data.deploy_data.profile.profile_settings.path
    );

    // `nix-store --query --deriver` doesn't work on invalid paths, so we parse output of show-derivation :(
    let mut show_derivation_command = Command::new("nix");

    show_derivation_command
        .arg("show-derivation")
        .arg(&data.deploy_data.profile.profile_settings.path);

    let show_derivation_output = show_derivation_command
        .output()
        .await
        .map_err(PushProfileError::ShowDerivationError)?;

    match show_derivation_output.status.code() {
        Some(0) => (),
        a => return Err(PushProfileError::ShowDerivationExitError(a)),
    };

    let derivation_info: HashMap<&str, serde_json::value::Value> = serde_json::from_str(
        std::str::from_utf8(&show_derivation_output.stdout)
            .map_err(PushProfileError::ShowDerivationUtf8Error)?,
    )
    .map_err(PushProfileError::ShowDerivationParseError)?;

    let derivation_name = derivation_info
        .keys()
        .next()
        .ok_or(PushProfileError::ShowDerivationEmpty)?;

    info!(
        "Building profile `{}` for node `{}`",
        data.deploy_data.profile_name, data.deploy_data.node_name
    );

    let mut build_command = if data.supports_flakes {
        Command::new("nix")
    } else {
        Command::new("nix-build")
    };

    if data.supports_flakes {
        build_command.arg("build").arg(derivation_name)
    } else {
        build_command.arg(derivation_name)
    };

    match (data.keep_result, data.supports_flakes) {
        (true, _) => {
            let result_path = data.result_path.unwrap_or("./.deploy-gc");

            build_command.arg("--out-link").arg(format!(
                "{}/{}/{}",
                result_path, data.deploy_data.node_name, data.deploy_data.profile_name
            ))
        }
        (false, false) => build_command.arg("--no-out-link"),
        (false, true) => build_command.arg("--no-link"),
    };

    for extra_arg in data.extra_build_args {
        build_command.arg(extra_arg);
    }

    let build_exit_status = build_command
        // Logging should be in stderr, this just stops the store path from printing for no reason
        .stdout(Stdio::null())
        .status()
        .await
        .map_err(PushProfileError::BuildError)?;

    match build_exit_status.code() {
        Some(0) => (),
        a => return Err(PushProfileError::BuildExitError(a)),
    };

    if !Path::new(
        format!(
            "{}/deploy-rs-activate",
            data.deploy_data.profile.profile_settings.path
        )
        .as_str(),
    )
    .exists()
    {
        return Err(PushProfileError::DeployRsActivateDoesntExist);
    }

    if !Path::new(
        format!(
            "{}/activate-rs",
            data.deploy_data.profile.profile_settings.path
        )
        .as_str(),
    )
    .exists()
    {
        return Err(PushProfileError::ActivateRsDoesntExist);
    }

    if let Ok(local_key) = std::env::var("LOCAL_KEY") {
        info!(
            "Signing key present! Signing profile `{}` for node `{}`",
            data.deploy_data.profile_name, data.deploy_data.node_name
        );

        let sign_exit_status = Command::new("nix")
            .arg("sign-paths")
            .arg("-r")
            .arg("-k")
            .arg(local_key)
            .arg(&data.deploy_data.profile.profile_settings.path)
            .status()
            .await
            .map_err(PushProfileError::SignError)?;

        match sign_exit_status.code() {
            Some(0) => (),
            a => return Err(PushProfileError::SignExitError(a)),
        };
    }

    info!(
        "Copying profile `{}` to node `{}`",
        data.deploy_data.profile_name, data.deploy_data.node_name
    );

    let mut copy_command = Command::new("nix");
    copy_command.arg("copy");

    if data.deploy_data.merged_settings.fast_connection != Some(true) {
        copy_command.arg("--substitute-on-destination");
    }

    if !data.check_sigs {
        copy_command.arg("--no-check-sigs");
    }

    let ssh_opts_str = data
        .deploy_data
        .merged_settings
        .ssh_opts
        // This should provide some extra safety, but it also breaks for some reason, oh well
        // .iter()
        // .map(|x| format!("'{}'", x))
        // .collect::<Vec<String>>()
        .join(" ");

    let hostname = match data.deploy_data.cmd_overrides.hostname {
        Some(ref x) => x,
        None => &data.deploy_data.node.node_settings.hostname,
    };

    let copy_exit_status = copy_command
        .arg("--to")
        .arg(format!("ssh://{}@{}", data.deploy_defs.ssh_user, hostname))
        .arg(&data.deploy_data.profile.profile_settings.path)
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
