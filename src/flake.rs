// SPDX-FileCopyrightText: 2020 Serokell <https://serokell.io/>
// SPDX-FileCopyrightText: 2021 Yannik Sander <contact@ysndr.de>
//
// SPDX-License-Identifier: MPL-2.0

use crate as deploy;

use self::deploy::{data, settings};
use log::{error, info};
use std::process::Stdio;
use futures_util::stream::{StreamExt, TryStreamExt};
use thiserror::Error;
use tokio::process::Command;

#[derive(Error, Debug)]
pub enum CheckDeploymentError {
    #[error("Failed to execute Nix checking command: {0}")]
    NixCheck(#[from] std::io::Error),
    #[error("Nix checking command resulted in a bad exit code: {0:?}")]
    NixCheckExit(Option<i32>),
}

pub async fn check_deployment(
    supports_flakes: bool,
    repo: &str,
    extra_build_args: &[String],
) -> Result<(), CheckDeploymentError> {
    info!("Running checks for flake in {}", repo);

    let mut check_command = match supports_flakes {
        true => Command::new("nix"),
        false => Command::new("nix-build"),
    };

    if supports_flakes {
        check_command.arg("flake").arg("check").arg(repo);
    } else {
        check_command.arg("-E")
            .arg("--no-out-link")
            .arg(format!("let r = import {}/.; x = (if builtins.isFunction r then (r {{}}) else r); in if x ? checks then x.checks.${{builtins.currentSystem}} else {{}}", repo));
    };

    for extra_arg in extra_build_args {
        check_command.arg(extra_arg);
    }

    let check_status = check_command.status().await?;

    match check_status.code() {
        Some(0) => (),
        a => return Err(CheckDeploymentError::NixCheckExit(a)),
    };

    Ok(())
}

#[derive(Error, Debug)]
pub enum GetDeploymentDataError {
    #[error("Failed to execute nix eval command: {0}")]
    NixEval(std::io::Error),
    #[error("Failed to read output from evaluation: {0}")]
    NixEvalOut(std::io::Error),
    #[error("Evaluation resulted in a bad exit code: {0:?}")]
    NixEvalExit(Option<i32>),
    #[error("Error converting evaluation output to utf8: {0}")]
    DecodeUtf8(#[from] std::string::FromUtf8Error),
    #[error("Error decoding the JSON from evaluation: {0}")]
    DecodeJson(#[from] serde_json::error::Error),
    #[error("Impossible happened: profile is set but node is not")]
    ProfileNoNode,
}

/// Evaluates the Nix in the given `repo` and return the processed Data from it
pub async fn get_deployment_data(
    supports_flakes: bool,
    flakes: &[data::Target],
    extra_build_args: &[String],
) -> Result<Vec<settings::Root>, GetDeploymentDataError> {
    futures_util::stream::iter(flakes).then(|flake| async move {

    info!("Evaluating flake in {}", flake.repo);

    let mut c = if supports_flakes {
        Command::new("nix")
    } else {
        Command::new("nix-instantiate")
    };

    if supports_flakes {
        c.arg("eval")
            .arg("--json")
            .arg(format!("{}#deploy", flake.repo))
            // We use --apply instead of --expr so that we don't have to deal with builtins.getFlake
            .arg("--apply");
        match (&flake.node, &flake.profile) {
            (Some(node), Some(profile)) => {
                // Ignore all nodes and all profiles but the one we're evaluating
                c.arg(format!(
                    r#"
                      deploy:
                      (deploy // {{
                        nodes = {{
                          "{0}" = deploy.nodes."{0}" // {{
                            profiles = {{
                              inherit (deploy.nodes."{0}".profiles) "{1}";
                            }};
                          }};
                        }};
                      }})
                     "#,
                    node, profile
                ))
            }
            (Some(node), None) => {
                // Ignore all nodes but the one we're evaluating
                c.arg(format!(
                    r#"
                      deploy:
                      (deploy // {{
                        nodes = {{
                          inherit (deploy.nodes) "{}";
                        }};
                      }})
                    "#,
                    node
                ))
            }
            (None, None) => {
                // We need to evaluate all profiles of all nodes anyway, so just do it strictly
                c.arg("deploy: deploy")
            }
            (None, Some(_)) => return Err(GetDeploymentDataError::ProfileNoNode),
        }
    } else {
        c
            .arg("--strict")
            .arg("--read-write-mode")
            .arg("--json")
            .arg("--eval")
            .arg("-E")
            .arg(format!("let r = import {}/.; in if builtins.isFunction r then (r {{}}).deploy else r.deploy", flake.repo))
    };

    for extra_arg in extra_build_args {
        c.arg(extra_arg);
    }

    let build_child = c
        .stdout(Stdio::piped())
        .spawn()
        .map_err(GetDeploymentDataError::NixEval)?;

    let build_output = build_child
        .wait_with_output()
        .await
        .map_err(GetDeploymentDataError::NixEvalOut)?;

    match build_output.status.code() {
        Some(0) => (),
        a => return Err(GetDeploymentDataError::NixEvalExit(a)),
    };

    let data_json = String::from_utf8(build_output.stdout)?;

    Ok(serde_json::from_str(&data_json)?)
}).try_collect().await
}
