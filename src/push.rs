// SPDX-FileCopyrightText: 2020 Serokell <https://serokell.io/>
//
// SPDX-License-Identifier: MPL-2.0

use log::{debug, info, warn};
use std::collections::HashMap;
use std::path::Path;
use std::process::Stdio;
use thiserror::Error;
use tokio::process::Command;

#[derive(Error, Debug)]
pub enum PushProfileError {
    #[error("Failed to run Nix show-derivation command: {0}")]
    ShowDerivation(std::io::Error),
    #[error("Nix show-derivation command resulted in a bad exit code: {0:?}")]
    ShowDerivationExit(Option<i32>),
    #[error("Nix show-derivation command output contained an invalid UTF-8 sequence: {0}")]
    ShowDerivationUtf8(std::str::Utf8Error),
    #[error("Failed to parse the output of nix show-derivation: {0}")]
    ShowDerivationParse(serde_json::Error),
    #[error("Nix show-derivation output is empty")]
    ShowDerivationEmpty,
    #[error("Failed to run Nix build command: {0}")]
    Build(std::io::Error),
    #[error("Nix build command resulted in a bad exit code: {0:?}")]
    BuildExit(Option<i32>),
    #[error(
        "Activation script deploy-rs-activate does not exist in profile.\n\
             Did you forget to use deploy-rs#lib.<...>.activate.<...> on your profile path?"
    )]
    DeployRsActivateDoesntExist,
    #[error("Activation script activate-rs does not exist in profile.\n\
             Is there a mismatch in deploy-rs used in the flake you're deploying and deploy-rs command you're running?")]
    ActivateRsDoesntExist,
    #[error("Failed to run Nix sign command: {0}")]
    Sign(std::io::Error),
    #[error("Nix sign command resulted in a bad exit code: {0:?}")]
    SignExit(Option<i32>),
    #[error("Failed to run Nix copy command: {0}")]
    Copy(std::io::Error),
    #[error("Nix copy command resulted in a bad exit code: {0:?}")]
    CopyExit(Option<i32>),

    #[error("Failed to run Nix path-info command: {0}")]
    PathInfo(std::io::Error),
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

pub async fn build_profile_locally(data: &PushProfileData<'_>, derivation_name: &str) -> Result<(), PushProfileError> {
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

    build_command.args(data.extra_build_args);

    let build_exit_status = build_command
        // Logging should be in stderr, this just stops the store path from printing for no reason
        .stdout(Stdio::null())
        .status()
        .await
        .map_err(PushProfileError::Build)?;

    match build_exit_status.code() {
        Some(0) => (),
        a => return Err(PushProfileError::BuildExit(a)),
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
            .map_err(PushProfileError::Sign)?;

        match sign_exit_status.code() {
            Some(0) => (),
            a => return Err(PushProfileError::SignExit(a)),
        };
    }
    Ok(())
}

pub async fn build_profile_remotely(data: &PushProfileData<'_>, derivation_name: &str) -> Result<(), PushProfileError> {
    info!(
        "Building profile `{}` for node `{}` on remote host",
        data.deploy_data.profile_name, data.deploy_data.node_name
    );

    // TODO: this should probably be handled more nicely during 'data' construction
    let hostname = match data.deploy_data.cmd_overrides.hostname {
        Some(ref x) => x,
        None => &data.deploy_data.node.node_settings.hostname,
    };
    let store_address = format!("ssh-ng://{}@{}", data.deploy_defs.ssh_user, hostname);

    let ssh_opts_str = data.deploy_data.merged_settings.ssh_opts.join(" ");


    // copy the derivation to remote host so it can be built there
    let copy_command_status = Command::new("nix")
        .arg("--experimental-features").arg("nix-command")
        .arg("copy")
        .arg("-s")  // fetch dependencies from substitures, not localhost
        .arg("--to").arg(&store_address)
        .arg("--derivation").arg(derivation_name)
        .env("NIX_SSHOPTS", ssh_opts_str.clone())
        .stdout(Stdio::null())
        .status()
        .await
        .map_err(PushProfileError::Copy)?;

    match copy_command_status.code() {
        Some(0) => (),
        a => return Err(PushProfileError::CopyExit(a)),
    };

    let mut build_command = Command::new("nix");
    build_command
        .arg("--experimental-features").arg("nix-command")
        .arg("build").arg(derivation_name)
        .arg("--eval-store").arg("auto")
        .arg("--store").arg(&store_address)
        .args(data.extra_build_args)
        .env("NIX_SSHOPTS", ssh_opts_str.clone());

    debug!("build command: {:?}", build_command);

    let build_exit_status = build_command
        // Logging should be in stderr, this just stops the store path from printing for no reason
        .stdout(Stdio::null())
        .status()
        .await
        .map_err(PushProfileError::Build)?;

    match build_exit_status.code() {
        Some(0) => (),
        a => return Err(PushProfileError::BuildExit(a)),
    };


    Ok(())
}

pub async fn build_profile(data: PushProfileData<'_>) -> Result<(), PushProfileError> {
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
        .map_err(PushProfileError::ShowDerivation)?;

    match show_derivation_output.status.code() {
        Some(0) => (),
        a => return Err(PushProfileError::ShowDerivationExit(a)),
    };

    let derivation_info: HashMap<&str, serde_json::value::Value> = serde_json::from_str(
        std::str::from_utf8(&show_derivation_output.stdout)
            .map_err(PushProfileError::ShowDerivationUtf8)?,
    )
    .map_err(PushProfileError::ShowDerivationParse)?;

    let deriver_key = derivation_info
        .keys()
        .next()
        .ok_or(PushProfileError::ShowDerivationEmpty)?;

    // Nix 2.32+ returns relative paths (without /nix/store/ prefix) in show-derivation output
    // Normalize to always use full store paths
    let deriver = if deriver_key.starts_with("/nix/store/") {
        deriver_key.to_string()
    } else {
        format!("/nix/store/{}", deriver_key)
    };

    let new_deriver = if data.supports_flakes || data.deploy_data.merged_settings.remote_build.unwrap_or(false) {
        // Since nix 2.15.0 'nix build <path>.drv' will build only the .drv file itself, not the
        // derivation outputs, '^out' is used to refer to outputs explicitly
        deriver.clone() + "^out"
    } else {
        deriver.clone()
    };

    let path_info_output = Command::new("nix")
        .arg("--experimental-features").arg("nix-command")
        .arg("path-info")
        .arg(&deriver)
        .output().await
        .map_err(PushProfileError::PathInfo)?;

    let deriver = if std::str::from_utf8(&path_info_output.stdout).map(|s| s.trim()) == Ok(deriver.as_str()) {
        // In this case we're on 2.15.0 or newer, because 'nix path-info <...>.drv'
        // returns the same '<...>.drv' path.
        // If 'nix path-info <...>.drv' returns a different path, then we're on pre 2.15.0 nix and
        // derivation build result is already present in the /nix/store.
        new_deriver
    } else {
        // If 'nix path-info <...>.drv' returns a different path, then we're on pre 2.15.0 nix and
        // derivation build result is already present in the /nix/store.
        //
        // Alternatively, the result of the derivation build may not be yet present
        // in the /nix/store. In this case, 'nix path-info' returns
        // 'error: path '...' is not valid'.
        deriver
    };
    if data.deploy_data.merged_settings.remote_build.unwrap_or(false) {
        if !data.supports_flakes {
            warn!("remote builds using non-flake nix are experimental");
        }

        build_profile_remotely(&data, &deriver).await?;
    } else {
        build_profile_locally(&data, &deriver).await?;
    }

    Ok(())
}

pub async fn push_profile(data: PushProfileData<'_>) -> Result<(), PushProfileError> {
    let ssh_opts_str = data
        .deploy_data
        .merged_settings
        .ssh_opts
        // This should provide some extra safety, but it also breaks for some reason, oh well
        // .iter()
        // .map(|x| format!("'{}'", x))
        // .collect::<Vec<String>>()
        .join(" ");

    // remote building guarantees that the resulting derivation is stored on the target system
    // no need to copy after building
    if !data.deploy_data.merged_settings.remote_build.unwrap_or(false) {
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
            .map_err(PushProfileError::Copy)?;

        match copy_exit_status.code() {
            Some(0) => (),
            a => return Err(PushProfileError::CopyExit(a)),
        };
    }

    Ok(())
}
