// SPDX-FileCopyrightText: 2020 Serokell <https://serokell.io/>
//
// SPDX-License-Identifier: MPL-2.0

use log::{debug, info};
use std::collections::HashMap;
use std::path::Path;
use std::process::Stdio;
use thiserror::Error;

use crate::command;

#[derive(Error, Debug)]
pub enum ShowDerivationError {
    #[error("Nix show-derivation command output contained an invalid UTF-8 sequence: {0}")]
    Utf8(std::str::Utf8Error),
    #[error("Failed to parse the output of nix show-derivation: {0}")]
    Parse(serde_json::Error),
    #[error("Nix show-derivation output is empty")]
    Empty,
}

impl command::HasCommandError for ShowDerivationError {
    fn title() -> String {
        "Nix show derivation".to_string()
    }
}

#[derive(Error, Debug)]
pub enum BuildError {}

impl command::HasCommandError for BuildError {
    fn title() -> String {
        "Nix build".to_string()
    }
}

#[derive(Error, Debug)]
pub enum SignError {}

impl command::HasCommandError for SignError {
    fn title() -> String {
        "Nix sign".to_string()
    }
}

#[derive(Error, Debug)]
pub enum CopyError {}

impl command::HasCommandError for CopyError {
    fn title() -> String {
        "Nix copy".to_string()
    }
}

#[derive(Error, Debug)]
pub enum PathInfoError {}

impl command::HasCommandError for PathInfoError {
    fn title() -> String {
        "Nix path-info".to_string()
    }
}

#[derive(Error, Debug)]
pub enum PushProfileError {
    #[error("{0}")]
    ShowDerivation(#[from] command::CommandError<ShowDerivationError>),
    #[error("{0}")]
    Build(#[from] command::CommandError<BuildError>),
    #[error(
        "Activation script deploy-rs-activate does not exist in profile.\n\
             Did you forget to use deploy-rs#lib.<...>.activate.<...> on your profile path?"
    )]
    DeployRsActivateDoesntExist,
    #[error("Activation script activate-rs does not exist in profile.\n\
             Is there a mismatch in deploy-rs used in the flake you're deploying and deploy-rs command you're running?")]
    ActivateRsDoesntExist,
    #[error("{0}")]
    Sign(#[from] command::CommandError<SignError>),
    #[error("{0}")]
    Copy(#[from] command::CommandError<CopyError>),
    #[error("The remote building option is not supported when using legacy nix")]
    RemoteBuildWithLegacyNix,
    #[error("{0}")]
    PathInfo(#[from] command::CommandError<PathInfoError>),
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
        command::Command::new("nix")
    } else {
        command::Command::new("nix-build")
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

    build_command
        // Logging should be in stderr, this just stops the store path from printing for no reason
        .stdout(Stdio::null())
        .run()
        .await
        .map_err(PushProfileError::Build)?;

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

        let mut sign_command = command::Command::new("nix");
        sign_command
            .arg("sign-paths")
            .arg("-r")
            .arg("-k")
            .arg(local_key)
            .arg(&data.deploy_data.profile.profile_settings.path)
            .run()
            .await
            .map_err(PushProfileError::Sign)?;
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
    let mut copy_command = command::Command::new("nix");
    copy_command
        .arg("copy")
        .arg("-s")  // fetch dependencies from substitures, not localhost
        .arg("--to").arg(&store_address)
        .arg("--derivation").arg(derivation_name)
        .env("NIX_SSHOPTS", ssh_opts_str.clone())
        .stdout(Stdio::null())
        .run()
        .await
        .map_err(PushProfileError::Copy)?;

    let mut build_command = command::Command::new("nix");
    build_command
        .arg("build").arg(derivation_name)
        .arg("--eval-store").arg("auto")
        .arg("--store").arg(&store_address)
        .args(data.extra_build_args)
        .env("NIX_SSHOPTS", ssh_opts_str.clone());

    debug!("build command: {:?}", build_command);

    build_command
        // Logging should be in stderr, this just stops the store path from printing for no reason
        .stdout(Stdio::null())
        .run()
        .await
        .map_err(PushProfileError::Build)?;

    Ok(())
}

pub async fn build_profile(data: PushProfileData<'_>) -> Result<(), PushProfileError> {
    debug!(
        "Finding the deriver of store path for {}",
        &data.deploy_data.profile.profile_settings.path
    );

    // `nix-store --query --deriver` doesn't work on invalid paths, so we parse output of show-derivation :(
    let mut show_derivation_command = command::Command::new("nix");

    show_derivation_command
        .arg("show-derivation")
        .arg(&data.deploy_data.profile.profile_settings.path);

    let show_derivation_output = show_derivation_command
        .run()
        .await
        .map_err(PushProfileError::ShowDerivation)?;

    let derivation_info: HashMap<&str, serde_json::value::Value> = serde_json::from_str(
        std::str::from_utf8(&show_derivation_output.stdout).map_err(|err| {
            PushProfileError::ShowDerivation(command::CommandError::OtherError(
                ShowDerivationError::Utf8(err)
            ))
        })?
    )
    .map_err(|err| {
        PushProfileError::ShowDerivation(command::CommandError::OtherError(
            ShowDerivationError::Parse(err)
        ))
    })?;

    let &deriver = derivation_info
        .keys()
        .next()
        .ok_or(
            PushProfileError::ShowDerivation(command::CommandError::OtherError(
                ShowDerivationError::Empty
            ))
        )?;

    let new_deriver = &if data.supports_flakes {
        // Since nix 2.15.0 'nix build <path>.drv' will build only the .drv file itself, not the
        // derivation outputs, '^out' is used to refer to outputs explicitly
        deriver.to_owned().to_string() + "^out"
    } else {
        deriver.to_owned()
    };

    let path_info_output = command::Command::new("nix")
        .arg("--experimental-features").arg("nix-command")
        .arg("path-info")
        .arg(&deriver)
        .run().await
        .map_err(PushProfileError::PathInfo)?;

    let deriver = if std::str::from_utf8(&path_info_output.stdout).map(|s| s.trim()) == Ok(deriver) {
        // In this case we're on 2.15.0 or newer, because 'nix path-infonix path-info <...>.drv'
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
            return Err(PushProfileError::RemoteBuildWithLegacyNix)
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

        let mut copy_command = command::Command::new("nix");
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

        copy_command
            .arg("--to")
            .arg(format!("ssh://{}@{}", data.deploy_defs.ssh_user, hostname))
            .arg(&data.deploy_data.profile.profile_settings.path)
            .env("NIX_SSHOPTS", ssh_opts_str)
            .run()
            .await
            .map_err(PushProfileError::Copy)?;
    }

    Ok(())
}
