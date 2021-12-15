// SPDX-FileCopyrightText: 2020 Serokell <https://serokell.io/>
//
// SPDX-License-Identifier: MPL-2.0

use log::{debug, info};
use std::collections::HashMap;
use std::path::Path;
use std::process::Stdio;
use thiserror::Error;
use tokio::process::Command;

use crate::data;

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

    #[error("Deployment data invalid: {0}")]
    DeployData(#[from] data::DeployDataError),
}

pub struct ShowDerivationCommand<'a> {
    closure: &'a str,
}

impl<'a> ShowDerivationCommand<'a> {
    pub fn from_data(d: &'a data::DeployData) -> Self {
        ShowDerivationCommand {
            closure: d.profile.profile_settings.path.as_str(),
        }
    }

    fn build(self) -> Command {
        // `nix-store --query --deriver` doesn't work on invalid paths, so we parse output of show-derivation :(
        let mut cmd = Command::new("nix");

        cmd
            .arg("show-derivation")
            .arg(&self.closure);
        //cmd.what_is_this;
        cmd
    }
}

pub struct SignCommand<'a> {
    closure: &'a str,
}

impl<'a> SignCommand<'a> {
    pub fn from_data(d: &'a data::DeployData) -> Self {
        SignCommand {
            closure: d.profile.profile_settings.path.as_str(),
        }
    }

    fn build(self, local_key: String) -> Command {
        let mut cmd = Command::new("nix");

        cmd
            .arg("sign-paths")
            .arg("-r")
            .arg("-k")
            .arg(local_key)
            .arg(&self.closure);
        //cmd.what_is_this;
        cmd
    }
}

pub struct CopyCommand<'a> {
    closure: &'a str,
    fast_connection: bool,
    check_sigs: &'a bool,
    ssh_uri: &'a str,
    ssh_opts: String,
}

impl<'a> CopyCommand<'a> {
    pub fn from_data(d: &'a data::DeployData) -> Self {
        CopyCommand {
            closure: d.profile.profile_settings.path.as_str(),
            fast_connection: d.merged_settings.fast_connection.unwrap_or(false),
            check_sigs: &d.flags.checksigs,
            ssh_uri: d.ssh_uri.as_str(),
            ssh_opts: d.merged_settings.ssh_opts.iter().fold("".to_string(), |s, o| format!("{} {}", s, o)),
        }
    }

    fn build(self) -> Command {
        let mut cmd = Command::new("nix");

        cmd.arg("copy");

        if self.fast_connection {
            cmd.arg("--substitute-on-destination");
        }

        if !self.check_sigs {
            cmd.arg("--no-check-sigs");
        }
        cmd
            .arg("--to")
            .arg(self.ssh_uri)
            .arg(self.closure)
            .env("NIX_SSHOPTS", self.ssh_opts);
        //cmd.what_is_this;
        cmd
    }
}

pub struct BuildCommand<'a> {
    node_name: &'a str,
    profile_name: &'a str,
    keep_result: &'a bool,
    result_path: &'a str,
    extra_build_args: &'a Vec<String>,
}

impl<'a> BuildCommand<'a> {
    pub fn from_data(d: &'a data::DeployData) -> Self {
        BuildCommand {
            node_name: d.node_name.as_str(),
            profile_name: d.profile_name.as_str(),
            keep_result: &d.flags.keep_result,
            result_path: &d.flags.result_path.as_deref().unwrap_or("./.deploy-gc"),
            extra_build_args: &d.flags.extra_build_args,
        }
    }

    fn build(self, derivation_name: &str, supports_flakes: bool) -> Command {
        let mut cmd = if supports_flakes {
            Command::new("nix")
        } else {
            Command::new("nix-build")
        };

        if supports_flakes {
            cmd.arg("build").arg(derivation_name)
        } else {
            cmd.arg(derivation_name)
        };

        match (self.keep_result, supports_flakes) {
            (true, _) => {
                cmd.arg("--out-link").arg(format!(
                    "{}/{}/{}",
                    self.result_path, self.node_name, self.profile_name
                ))
            }
            (false, false) => cmd.arg("--no-out-link"),
            (false, true) => cmd.arg("--no-link"),
        };
        cmd.args(self.extra_build_args.iter());
        // cmd.what_is_this;
        cmd
    }
}

pub async fn push_profile(
    supports_flakes: bool,
    show_derivation: ShowDerivationCommand<'_>,
    build: BuildCommand<'_>,
    sign: SignCommand<'_>,
    copy: CopyCommand<'_>,
) -> Result<(), PushProfileError> {
    let node_name = build.node_name;
    let profile_name = build.profile_name;
    let closure = show_derivation.closure;

    debug!("Finding the deriver of store path for {}", closure);
    let mut show_derivation_cmd = show_derivation.build();

    let show_derivation_output = show_derivation_cmd
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

    let derivation_name = derivation_info
        .keys()
        .next()
        .ok_or(PushProfileError::ShowDerivationEmpty)?;

    info!("Building profile `{}` for node `{}`", profile_name, node_name);

    let mut build_cmd = build.build(*derivation_name, supports_flakes);

    let build_exit_status = build_cmd
        // Logging should be in stderr, this just stops the store path from printing for no reason
        .stdout(Stdio::null())
        .status()
        .await
        .map_err(PushProfileError::Build)?;

    match build_exit_status.code() {
        Some(0) => (),
        a => return Err(PushProfileError::BuildExit(a)),
    };

    if !Path::new(format!("{}/deploy-rs-activate", closure).as_str()).exists() {
        return Err(PushProfileError::DeployRsActivateDoesntExist);
    }

    if !Path::new(format!("{}/activate-rs", closure).as_str()).exists() {
        return Err(PushProfileError::ActivateRsDoesntExist);
    }

    if let Ok(local_key) = std::env::var("LOCAL_KEY") {
        info!("Signing key present! Signing profile `{}` for node `{}`", profile_name, node_name);

        let mut sign_cmd = sign.build(local_key);
        let sign_exit_status = sign_cmd
            .status()
            .await
            .map_err(PushProfileError::Sign)?;

        match sign_exit_status.code() {
            Some(0) => (),
            a => return Err(PushProfileError::SignExit(a)),
        };
    }

    info!("Copying profile `{}` to node `{}`", profile_name, node_name);

    let mut copy_cmd = copy.build();

    let copy_exit_status = copy_cmd
        .status()
        .await
        .map_err(PushProfileError::Copy)?;

    match copy_exit_status.code() {
        Some(0) => (),
        a => return Err(PushProfileError::CopyExit(a)),
    };

    Ok(())
}
