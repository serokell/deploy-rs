// SPDX-FileCopyrightText: 2020 Serokell <https://serokell.io/>
//
// SPDX-License-Identifier: MPL-2.0

use indicatif::ProgressBar;
use log::{debug, info, warn};
use std::collections::HashMap;
use std::path::Path;
use std::process::Stdio;
use thiserror::Error;
use tokio::io::AsyncBufReadExt;
use tokio::io::BufReader;
use tokio::process::Child;
use tokio::process::Command;
use tokio_stream::wrappers::LinesStream;
use tokio_stream::StreamExt;

use crate::command;

#[derive(Error, Debug)]
pub enum ShowDerivationError {
    #[error("Nix show-derivation command output contained an invalid UTF-8 sequence: {0}")]
    Utf8(std::str::Utf8Error),
    #[error("Failed to parse the output of nix show-derivation: {0}")]
    Parse(serde_json::Error),
    #[error("Nix show derivation output is not an object")]
    Invalid,
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
    #[error("{0}")]
    Sign(#[from] command::CommandError<SignError>),
    #[error("{0}")]
    Copy(#[from] command::CommandError<CopyError>),
    #[error("{0}")]
    PathInfo(#[from] command::CommandError<PathInfoError>),
    #[error("Copy exited with status {}", .0.map(|c| c.to_string()).unwrap_or_else(|| "unknown".to_string()))]
    CopyExit(Option<i32>),
    #[error("Failed to run Nix eval command: {0}")]
    EvalStore(std::io::error),
    #[error("Failed to convert nix store to utf8: {}", .0.map(|c| c.to_string()).unwrap_or_else(|| "unknown".to_string()))]
    EvalStoreUtf8(std::io::error),
    #[error("Build exited with status {}", .0.map(|c| c.to_string()).unwrap_or_else(|| "unknown".to_string()))]
    BuildExit(Option<i32>),
    #[error(
        "Activation script deploy-rs-activate does not exist in profile.\n\
             Did you forget to use deploy-rs#lib.<...>.activate.<...> on your profile path?"
    )]
    DeployRsActivateDoesntExist,
    #[error(
        "Activation script activate-rs does not exist in profile.\n\
             Is there a mismatch in deploy-rs used in the flake you're deploying and deploy-rs command you're running?"
    )]
    ActivateRsDoesntExist,
    #[error("Nix build command output contained an invalid UTF-8 sequence: {0}")]
    BuildStdoutUtf8(std::str::Utf8Error),
    #[error("Nix build command succeeded but printed no output path")]
    BuildStdoutEmpty,
    #[error("Nix build command printed multiple output paths, expected exactly one: {0}")]
    BuildStdoutMultiline(String),
}

#[derive(Clone)]
pub struct PushProfileData {
    pub supports_flakes: bool,
    pub check_sigs: bool,
    pub repo: String,
    pub deploy_data: super::DeployData,
    pub deploy_defs: super::DeployDefs,
    pub keep_result: bool,
    pub result_path: Option<String>,
    pub extra_build_args: Vec<String>,
}

pub async fn build_profile_locally(
    data: &PushProfileData,
    derivation_name: &str,
) -> Result<String, PushProfileError> {
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
        // `--print-out-paths` makes `nix build` write the realised output
        // to stdout. `nix-build` writes the path to stdout by default so
        // the flag only applies to the flake branch.
        build_command
            .arg("build")
            .arg(derivation_name)
            .arg("--print-out-paths")
    } else {
        build_command.arg(derivation_name)
    };

    match (data.keep_result, data.supports_flakes) {
        (true, _) => {
            let result_path = data
                .result_path
                .clone()
                .unwrap_or("./.deploy-gc".to_string());

            build_command.arg("--out-link").arg(format!(
                "{}/{}/{}",
                result_path, data.deploy_data.node_name, data.deploy_data.profile_name
            ))
        }
        (false, false) => build_command.arg("--no-out-link"),
        (false, true) => build_command.arg("--no-link"),
    };

    build_command.args(data.extra_build_args.clone());

    debug!("build command: {:?}", build_command);

    // Capture stdout so we can read the realised store path. When a progress
    // bar is attached, stderr is also piped and routed through the spinner's
    // message via `update_pb_with_child_output`; otherwise stderr is left
    // inherited so nix's native build logs stream straight to the terminal.
    let build_output = if let Some(pb) = &data.deploy_data.progressbar {
        // `internal-json` gives us structured progress to render in the spinner
        // instead of nix's own (terminal-drawing) progress bar.
        build_command.arg("--log-format").arg("internal-json");
        let mut child = build_command
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| PushProfileError::Build(command::CommandError::RunError(e)))?;

        // The realised store path arrives on stdout as a plain line, while
        // only stderr carries the `@nix ...` progress events, so read stdout
        // ourselves while the spinner consumes stderr.
        let mut stdout = child.stdout.take().expect("stdout was piped");
        let stdout_task = tokio::spawn(async move {
            let mut buf = Vec::new();
            tokio::io::AsyncReadExt::read_to_end(&mut stdout, &mut buf)
                .await
                .map(|_| buf)
        });

        update_pb_with_child_output(pb, &mut child).await;

        let status = child
            .wait()
            .await
            .map_err(|e| PushProfileError::Build(command::CommandError::RunError(e)))?;
        let stdout = stdout_task
            .await
            .map_err(|e| {
                PushProfileError::Build(command::CommandError::RunError(std::io::Error::other(e)))
            })?
            .map_err(|e| PushProfileError::Build(command::CommandError::RunError(e)))?;

        std::process::Output {
            status,
            stdout,
            stderr: Vec::new(),
        }
    } else {
        let child = build_command
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .map_err(|e| PushProfileError::Build(command::CommandError::RunError(e)))?;
        child
            .wait_with_output()
            .await
            .map_err(|e| PushProfileError::Build(command::CommandError::RunError(e)))?
    };

    match build_output.status.code() {
        Some(0) => (),
        a => return Err(PushProfileError::BuildExit(a)),
    };

    let closure = parse_build_out_path(&build_output.stdout)?;

    if !Path::new(format!("{}/deploy-rs-activate", closure).as_str()).exists() {
        return Err(PushProfileError::DeployRsActivateDoesntExist);
    }

    if !Path::new(format!("{}/activate-rs", closure).as_str()).exists() {
        return Err(PushProfileError::ActivateRsDoesntExist);
    }

    if let Ok(local_key) = std::env::var("LOCAL_KEY") {
        info!(
            "Signing key present! Signing profile `{}` for node `{}`",
            data.deploy_data.profile_name, data.deploy_data.node_name
        );

        let mut sign_command = Command::new("nix");
        sign_command
            .arg("sign-paths")
            .arg("-r")
            .arg("-k")
            .arg(local_key)
            .arg(&closure);
        command::Command::new(sign_command)
            .status()
            .await
            .map_err(PushProfileError::Sign)?;
    }
    Ok(closure)
}

// Nix `internal-json` activity types (see nix's `logging.hh`).
const ACT_FILE_TRANSFER: i64 = 101;
const ACT_COPY_PATHS: i64 = 103;
const ACT_BUILDS: i64 = 104;
const RES_BUILD_LOG_LINE: i64 = 101;
const RES_PROGRESS: i64 = 105;
// Nix verbosity levels: 0 = error, 1 = warning (higher = notice/info/debug).
const LVL_WARN: i64 = 1;

/// Accumulates the aggregate progress reported by `nix --log-format internal-json`
/// so we can render a compact `x/y built, x/y copied` message next to the spinner,
/// mirroring nix's own progress bar. Only used when builds run concurrently (i.e.
/// remote builds); local builds print nix's native output directly.
#[derive(Default)]
struct NixProgress {
    // Map of activity id -> activity type, so `result` events can be attributed.
    activities: HashMap<u64, i64>,
    builds: (u64, u64),
    copies: (u64, u64),
    // Downloads are not an aggregate activity: each file transfer reports its
    // own byte count, so we track the latest bytes per active transfer and keep
    // a running total of finished ones to render a single `X MiB DL` figure.
    download_active: HashMap<u64, u64>,
    download_done: u64,
    // An error/warning message from the last ingested event, always surfaced so
    // failures aren't reduced to a bare exit code.
    pending_msg: Option<String>,
    // The most recent human-readable activity text (current path, build log line, ...).
    last_line: String,
}

/// Render a byte count the way nix's own progress bar does (KiB/MiB/GiB, base 1024).
fn human_bytes(n: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KiB", "MiB", "GiB", "TiB"];
    let mut value = n as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{} {}", n, UNITS[unit])
    } else {
        format!("{:.1} {}", value, UNITS[unit])
    }
}

impl NixProgress {
    /// Ingest one line of child output. Returns `true` if it was a structured
    /// `@nix` message (already handled), `false` if it is a plain log line the
    /// caller should surface directly.
    fn ingest(&mut self, line: &str) -> bool {
        let json = match line.strip_prefix("@nix ") {
            Some(json) => json,
            None => return false,
        };
        let value: serde_json::Value = match serde_json::from_str(json) {
            Ok(value) => value,
            Err(_) => return false,
        };

        let action = value.get("action").and_then(|a| a.as_str()).unwrap_or("");
        match action {
            "start" => {
                let id = value.get("id").and_then(serde_json::Value::as_u64);
                let typ = value.get("type").and_then(serde_json::Value::as_i64);
                if let (Some(id), Some(typ)) = (id, typ) {
                    self.activities.insert(id, typ);
                }
                // Show the individual operation (copying/downloading/building '...').
                if let Some(text) = value.get("text").and_then(serde_json::Value::as_str) {
                    if !text.is_empty() {
                        self.last_line = text.to_string();
                    }
                }
            }
            "stop" => {
                if let Some(id) = value.get("id").and_then(serde_json::Value::as_u64) {
                    // Fold a finished download's bytes into the running total.
                    if let Some(bytes) = self.download_active.remove(&id) {
                        self.download_done += bytes;
                    }
                    self.activities.remove(&id);
                }
            }
            "result" => {
                let id = value.get("id").and_then(serde_json::Value::as_u64);
                let rtype = value.get("type").and_then(serde_json::Value::as_i64);
                let fields = value.get("fields").and_then(serde_json::Value::as_array);
                match rtype {
                    // Progress on an aggregate activity: fields = [done, expected, running, failed].
                    Some(RES_PROGRESS) => {
                        if let (Some(&atype), Some(fields)) =
                            (id.and_then(|id| self.activities.get(&id)), fields)
                        {
                            let done = fields.first().and_then(serde_json::Value::as_u64);
                            let expected = fields.get(1).and_then(serde_json::Value::as_u64);
                            match atype {
                                ACT_BUILDS => {
                                    if let (Some(done), Some(expected)) = (done, expected) {
                                        self.builds = (done, expected);
                                    }
                                }
                                ACT_COPY_PATHS => {
                                    if let (Some(done), Some(expected)) = (done, expected) {
                                        self.copies = (done, expected);
                                    }
                                }
                                // fields[0] is this transfer's downloaded bytes.
                                ACT_FILE_TRANSFER => {
                                    if let (Some(id), Some(done)) = (id, done) {
                                        self.download_active.insert(id, done);
                                    }
                                }
                                _ => {}
                            }
                        }
                    }
                    // A line of build output from a running derivation: shown as
                    // the spinner's current-activity text.
                    Some(RES_BUILD_LOG_LINE) => {
                        if let Some(text) = fields
                            .and_then(|f| f.first())
                            .and_then(serde_json::Value::as_str)
                        {
                            if !text.is_empty() {
                                self.last_line = text.to_string();
                            }
                        }
                    }
                    _ => {}
                }
            }
            // Errors and warnings: always surfaced (independent of `-L`) so a
            // failed build shows its reason instead of a bare exit code.
            "msg" => {
                let level = value
                    .get("level")
                    .and_then(serde_json::Value::as_i64)
                    .unwrap_or(i64::MAX);
                if level <= LVL_WARN {
                    if let Some(msg) = value.get("msg").and_then(serde_json::Value::as_str) {
                        if !msg.is_empty() {
                            self.pending_msg = Some(msg.to_string());
                        }
                    }
                }
            }
            _ => {}
        }
        true
    }

    /// Render the accumulated state into a spinner message.
    fn message(&self) -> String {
        let mut counts = Vec::new();
        if self.builds.1 > 0 {
            counts.push(format!("{}/{} built", self.builds.0, self.builds.1));
        }
        if self.copies.1 > 0 {
            counts.push(format!("{}/{} copied", self.copies.0, self.copies.1));
        }
        let downloaded: u64 = self.download_done + self.download_active.values().sum::<u64>();
        if downloaded > 0 {
            counts.push(format!("{} DL", human_bytes(downloaded)));
        }
        let counts = counts.join(", ");

        match (counts.is_empty(), self.last_line.is_empty()) {
            (false, false) => format!("[{}] {}", counts, self.last_line),
            (false, true) => counts,
            (true, false) => self.last_line.clone(),
            (true, true) => "...".to_string(),
        }
    }
}

async fn update_pb_with_child_output(pb: &ProgressBar, child: &mut Child) {
    // Only follow the streams that were actually piped. Some callers null out
    // stdout (e.g. local builds, where nix prints the store path there), so we
    // must not assume both handles are present.
    let stdout = child
        .stdout
        .take()
        .map(|out| LinesStream::new(BufReader::new(out).lines()));
    let stderr = child
        .stderr
        .take()
        .map(|err| LinesStream::new(BufReader::new(err).lines()));

    let mut merged = match (stdout, stderr) {
        (Some(out), Some(err)) => Box::pin(StreamExt::merge(out, err))
            as std::pin::Pin<Box<dyn tokio_stream::Stream<Item = _> + Send>>,
        (Some(out), None) => Box::pin(out),
        (None, Some(err)) => Box::pin(err),
        (None, None) => return,
    };

    let mut progress = NixProgress::default();
    while let Some(line) = merged.next().await {
        let line = line.expect("expected a valid line");
        // Structured `@nix` events feed the aggregate counters; anything else
        // (e.g. a stray warning) is shown verbatim.
        if progress.ingest(&line) {
            // Errors/warnings are always surfaced above the bar.
            if let Some(msg) = progress.pending_msg.take() {
                pb.println(msg);
            }
            pb.set_message(progress.message());
        } else if !line.is_empty() {
            pb.set_message(line);
        }
    }
}

pub async fn build_profile_remotely(
    data: &PushProfileData,
    derivation_name: &str,
) -> Result<String, PushProfileError> {
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
    let copy_command_status = {
        let mut copy_command = Command::new("nix");
        copy_command
            .arg("copy")
            .arg("-s") // fetch dependencies from substitures, not localhost
            .arg("--to")
            .arg(&store_address)
            .arg("--derivation")
            .arg(derivation_name)
            .env("NIX_SSHOPTS", ssh_opts_str.clone());

        if let Some(pb) = &data.deploy_data.progressbar {
            // Concurrent deploy: route nix's output through the spinner.
            copy_command.arg("--log-format").arg("internal-json");
            debug!("copy command: {:?}", copy_command);
            let mut child = copy_command
                .stderr(Stdio::piped())
                .stdout(Stdio::piped())
                .spawn()
                .expect("failed to spawn nix copy command");

            update_pb_with_child_output(pb, &mut child).await;

            child
                .wait()
                .await
                .map_err(|e| PushProfileError::Copy(command::CommandError::RunError(e)))?
        } else {
            // No progress bar: let nix write its native output to the terminal.
            debug!("copy command: {:?}", copy_command);
            copy_command
                .status()
                .await
                .map_err(|e| PushProfileError::Copy(command::CommandError::RunError(e)))?
        }
    };

    match copy_command_status.code() {
        Some(0) => (),
        a => return Err(PushProfileError::CopyExit(a)),
    };

    let build_output = {
        let mut build_command = Command::new("nix");
        build_command
            .arg("build")
            .arg(derivation_name)
            .arg("--eval-store")
            .arg("auto")
            .arg("--store")
            .arg(&store_address)
            .arg("--print-out-paths")
            .args(data.extra_build_args.clone())
            .env("NIX_SSHOPTS", ssh_opts_str.clone());

        if let Some(pb) = &data.deploy_data.progressbar {
            // Concurrent deploy: route nix's output through the spinner.
            build_command.arg("--log-format").arg("internal-json");
            debug!("build command: {:?}", build_command);
            let mut child = build_command
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .expect("failed to spawn nix build command");

            // The realised store path arrives on stdout as a plain line,
            // while only stderr carries the `@nix ...` progress events, so
            // read stdout ourselves while the spinner consumes stderr.
            let mut stdout = child.stdout.take().expect("stdout was piped");
            let stdout_task = tokio::spawn(async move {
                let mut buf = Vec::new();
                tokio::io::AsyncReadExt::read_to_end(&mut stdout, &mut buf)
                    .await
                    .map(|_| buf)
            });

            update_pb_with_child_output(pb, &mut child).await;

            let status = child
                .wait()
                .await
                .map_err(|e| PushProfileError::Build(command::CommandError::RunError(e)))?;
            let stdout = stdout_task
                .await
                .map_err(|e| {
                    PushProfileError::Build(command::CommandError::RunError(std::io::Error::other(
                        e,
                    )))
                })?
                .map_err(|e| PushProfileError::Build(command::CommandError::RunError(e)))?;

            std::process::Output {
                status,
                stdout,
                stderr: Vec::new(),
            }
        } else {
            // No progress bar: let nix write its native output to the terminal.
            debug!("build command: {:?}", build_command);
            let child = build_command
                .stdout(Stdio::piped())
                .stderr(Stdio::inherit())
                .spawn()
                .map_err(|e| PushProfileError::Build(command::CommandError::RunError(e)))?;
            child
                .wait_with_output()
                .await
                .map_err(|e| PushProfileError::Build(command::CommandError::RunError(e)))?
        }
    };

    match build_output.status.code() {
        Some(0) => (),
        a => return Err(PushProfileError::BuildExit(a)),
    };

    parse_build_out_path(&build_output.stdout)
}

pub async fn build_profile(data: &PushProfileData) -> Result<String, PushProfileError> {
    let profile_settings = &data.deploy_data.profile.profile_settings;

    let supports_caret = data.supports_flakes
        || data
            .deploy_data
            .merged_settings
            .remote_build
            .unwrap_or(false);

    // The eval transformation in `nix/transform-deploy.nix` attaches `drvPath`
    // to every derivation-typed profile path, so this branch is hit whenever
    // the user's `path` resolves to a derivation. Using `drvPath` directly also
    // bypasses `nix show-derivation`, which cannot resolve floating-output
    // placeholder paths. The legacy branch below remains for the case where
    // the user wrote a literal store path string in their `deploy` attribute.
    let deriver = if let Some(drv_path) = &profile_settings.drv_path {
        debug!("Using drvPath from flake: {}", drv_path);
        deriver_for_build(drv_path.clone(), supports_caret).await?
    } else {
        debug!(
            "Finding the deriver of store path for {}",
            &profile_settings.path
        );

        // `nix-store --query --deriver` doesn't work on invalid paths, so we parse output of show-derivation :(
        let mut show_derivation_command = Command::new("nix");
        show_derivation_command
            .arg("--experimental-features")
            .arg("nix-command")
            .arg("show-derivation")
            .arg(&profile_settings.path);
        let show_derivation_command_str = format!("{:?}", show_derivation_command);

        let show_derivation_output = command::Command::new(show_derivation_command)
            .run()
            .await
            .map_err(PushProfileError::ShowDerivation)?;

        match show_derivation_output.status.code() {
            Some(0) => (),
            _exit_code => {
                return Err(PushProfileError::ShowDerivation(
                    command::CommandError::Exit(
                        show_derivation_output,
                        show_derivation_command_str,
                    ),
                ));
            }
        };

        let show_derivation_json: serde_json::value::Value = serde_json::from_str(
            std::str::from_utf8(&show_derivation_output.stdout).map_err(|err| {
                PushProfileError::ShowDerivation(command::CommandError::OtherError(
                    ShowDerivationError::Utf8(err),
                ))
            })?,
        )
        .map_err(|err| {
            PushProfileError::ShowDerivation(command::CommandError::OtherError(
                ShowDerivationError::Parse(err),
            ))
        })?;

        // Nix 2.33+ nests derivations under a "derivations" key, so try to get that first
        let derivation_info = show_derivation_json
            .get("derivations")
            .unwrap_or(&show_derivation_json)
            .as_object()
            .ok_or(PushProfileError::ShowDerivation(
                command::CommandError::OtherError(ShowDerivationError::Invalid),
            ))?;

        let deriver_key = derivation_info
            .keys()
            .next()
            .ok_or(PushProfileError::ShowDerivation(
                command::CommandError::OtherError(ShowDerivationError::Empty),
            ))?;

        // Nix 2.32+ returns relative paths (without /nix/store/ prefix) in show-derivation output
        // Normalize to always use full store paths
        let nix_store_output = Command::new("nix")
            .arg("eval")
            .arg("--raw")
            .arg("--expr")
            .arg("builtins.storeDir")
            .output().await
            .map_err(PushProfileError::EvalStore)?;
        let nix_store = std::str::from_utf8(&nix_store_output.stdout).map_err(PushProfileError::EvalStoreUtf8)?;

        let deriver = if deriver_key.starts_with(nix_store) {
            deriver_key.to_string()
        } else {
            format!("{}/{}", nix_store, deriver_key)
        };

        deriver_for_build(deriver, supports_caret).await?
    };

    if data
        .deploy_data
        .merged_settings
        .remote_build
        .unwrap_or(false)
    {
        if !data.supports_flakes {
            warn!("remote builds using non-flake nix are experimental");
        }

        build_profile_remotely(data, &deriver).await
    } else {
        build_profile_locally(data, &deriver).await
    }
}

/// Picks the `nix build` argument shape for a given deriver, accounting for the
/// pre/post 2.15 split: on 2.15 and newer, `nix build <drv>` builds only the
/// `.drv` itself and `^out` is needed to select outputs; on older Nix,
/// `nix build <drv>` already builds outputs and `^out` is not understood. We
/// detect which case applies by asking `nix path-info <drv>`; on 2.15 and newer
/// it echoes the `.drv` back, while on older versions it resolves to the
/// realised output or errors out if the output is not yet built.
async fn deriver_for_build(
    deriver: String,
    supports_caret: bool,
) -> Result<String, PushProfileError> {
    if !supports_caret {
        return Ok(deriver);
    }

    let mut path_info_command = Command::new("nix");
    path_info_command
        .arg("--experimental-features")
        .arg("nix-command")
        .arg("path-info")
        .arg(&deriver);
    let path_info_output = command::Command::new(path_info_command)
        .run()
        .await
        .map_err(PushProfileError::PathInfo)?;

    if std::str::from_utf8(&path_info_output.stdout).map(|s| s.trim()) == Ok(deriver.as_str()) {
        Ok(format!("{}^out", deriver))
    } else {
        Ok(deriver)
    }
}

/// Extracts the realised `/nix/store/...` path from `nix build`'s stdout.
///
/// Both `nix build --print-out-paths` and `nix-build` write one path per
/// line. A deploy-rs build asks for exactly one output, so anything other
/// than a single non-empty line is rejected rather than silently truncated.
fn parse_build_out_path(stdout: &[u8]) -> Result<String, PushProfileError> {
    let text = std::str::from_utf8(stdout).map_err(PushProfileError::BuildStdoutUtf8)?;
    let trimmed = text.trim();

    if trimmed.is_empty() {
        return Err(PushProfileError::BuildStdoutEmpty);
    }
    if trimmed.contains('\n') {
        return Err(PushProfileError::BuildStdoutMultiline(trimmed.to_string()));
    }

    debug!("Built closure {}", trimmed);
    Ok(trimmed.to_string())
}

pub async fn push_profile(data: &PushProfileData, closure: &str) -> Result<(), PushProfileError> {
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
    if !data
        .deploy_data
        .merged_settings
        .remote_build
        .unwrap_or(false)
    {
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

        copy_command
            .arg("--to")
            .arg(format!("ssh://{}@{}", data.deploy_defs.ssh_user, hostname))
            .arg(closure)
            .env("NIX_SSHOPTS", ssh_opts_str);

        debug!("copy command: {:?}", copy_command);

        let copy_exit_status = if let Some(pb) = &data.deploy_data.progressbar {
            // A progress bar is attached (concurrent deploy): pipe nix's output
            // and route it through the spinner instead of letting it draw to the
            // terminal (which would corrupt the bars).
            copy_command.arg("--log-format").arg("internal-json");
            let mut child = copy_command
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .expect("failed to spawn nix copy command");

            update_pb_with_child_output(pb, &mut child).await;

            child
                .wait()
                .await
                .map_err(|e| PushProfileError::Copy(command::CommandError::RunError(e)))?
        } else {
            // No progress bar: let nix write its native output to the terminal.
            copy_command
                .status()
                .await
                .map_err(|e| PushProfileError::Copy(command::CommandError::RunError(e)))?
        };

        match copy_exit_status.code() {
            Some(0) => (),
            a => return Err(PushProfileError::CopyExit(a)),
        };
    }

    Ok(())
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn parse_build_out_path_returns_single_line() {
        // The happy path: `nix build --print-out-paths` writes exactly one
        // store path followed by a newline.
        let stdout = b"/nix/store/abc123-example\n";
        let path = parse_build_out_path(stdout).expect("single-line stdout must parse");
        assert_eq!(path, "/nix/store/abc123-example");
    }

    #[test]
    fn parse_build_out_path_rejects_empty_output() {
        // `nix build` can exit 0 without emitting a realised path under some
        // dynamic-derivation failure modes. Surfacing the missing closure
        // here keeps it from being silently fed into a downstream
        // activate-script check as an empty string.
        let err = parse_build_out_path(b"").expect_err("empty stdout must error");
        assert!(matches!(err, PushProfileError::BuildStdoutEmpty));
    }

    #[test]
    fn parse_build_out_path_rejects_multiple_outputs() {
        // deploy-rs builds exactly one `out` per profile, so more than one
        // line on stdout means our invocation has drifted from what the rest
        // of the pipeline expects. Refuse rather than silently picking one.
        let stdout = b"/nix/store/a\n/nix/store/b\n";
        let err = parse_build_out_path(stdout).expect_err("multiline stdout must error");
        assert!(matches!(err, PushProfileError::BuildStdoutMultiline(_)));
    }
}
