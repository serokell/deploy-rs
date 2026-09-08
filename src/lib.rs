// SPDX-FileCopyrightText: 2020 Serokell <https://serokell.io/>
// SPDX-FileCopyrightText: 2020 Andreas Fuchs <asf@boinkor.net>
// SPDX-FileCopyrightText: 2021 Yannik Sander <contact@ysndr.de>
//
// SPDX-License-Identifier: MPL-2.0

use indicatif::MultiProgress;
use rnix::{types::*, SyntaxKind::*};

use merge::Merge;

use thiserror::Error;

use flexi_logger::*;

use std::path::{Path, PathBuf};

pub fn make_lock_path(temp_path: &Path, closure: &str) -> PathBuf {
    let lock_hash = &closure["/nix/store/".len()..closure.find('-').unwrap_or(closure.len())];
    temp_path.join(format!("deploy-rs-canary-{}", lock_hash))
}

pub fn make_cancel_path(temp_path: &Path, closure: &str) -> PathBuf {
    let lock_hash = &closure["/nix/store/".len()..closure.find('-').unwrap_or(closure.len())];
    temp_path.join(format!("deploy-rs-cancel-{}", lock_hash))
}

#[cfg(test)]
mod sentinel_path_tests {
    use super::*;

    #[test]
    fn canary_and_cancel_paths_are_distinct() {
        let temp_path = Path::new("/tmp");
        let closure = "/nix/store/somehash-test";

        assert_eq!(
            make_lock_path(temp_path, closure),
            PathBuf::from("/tmp/deploy-rs-canary-somehash")
        );
        assert_eq!(
            make_cancel_path(temp_path, closure),
            PathBuf::from("/tmp/deploy-rs-cancel-somehash")
        );
    }

    #[test]
    fn cancel_path_handles_a_closure_without_a_name() {
        assert_eq!(
            make_cancel_path(Path::new("/tmp"), "/nix/store/somehash"),
            PathBuf::from("/tmp/deploy-rs-cancel-somehash")
        );
    }
}

const fn make_emoji(level: log::Level) -> &'static str {
    match level {
        log::Level::Error => "❌",
        log::Level::Warn => "⚠️",
        log::Level::Info => "ℹ️",
        log::Level::Debug => "❓",
        log::Level::Trace => "🖊️",
    }
}

pub fn logger_formatter_activate(
    w: &mut dyn std::io::Write,
    _now: &mut DeferredNow,
    record: &Record,
) -> Result<(), std::io::Error> {
    let level = record.level();

    write!(
        w,
        "⭐ {} [activate] [{}] {}",
        make_emoji(level),
        style(level, level.to_string()),
        record.args()
    )
}

pub fn logger_formatter_wait(
    w: &mut dyn std::io::Write,
    _now: &mut DeferredNow,
    record: &Record,
) -> Result<(), std::io::Error> {
    let level = record.level();

    write!(
        w,
        "👀 {} [wait] [{}] {}",
        make_emoji(level),
        style(level, level.to_string()),
        record.args()
    )
}

pub fn logger_formatter_revoke(
    w: &mut dyn std::io::Write,
    _now: &mut DeferredNow,
    record: &Record,
) -> Result<(), std::io::Error> {
    let level = record.level();

    write!(
        w,
        "↩️ {} [revoke] [{}] {}",
        make_emoji(level),
        style(level, level.to_string()),
        record.args()
    )
}

pub fn logger_formatter_deploy(
    w: &mut dyn std::io::Write,
    _now: &mut DeferredNow,
    record: &Record,
) -> Result<(), std::io::Error> {
    let level = record.level();

    write!(
        w,
        "🚀 {} [deploy] [{}] {}",
        make_emoji(level),
        style(level, level.to_string()),
        record.args()
    )
}

pub enum LoggerType {
    Deploy,
    Activate,
    Wait,
    Revoke,
}

use log::Log;

pub struct LogWrapper {
    bar: MultiProgress,
    log: Box<dyn Log>,
}

impl LogWrapper {
    pub fn new(bar: MultiProgress, log: Box<dyn Log>) -> Self {
        Self { bar, log }
    }

    pub fn try_init(self) -> Result<(), log::SetLoggerError> {
        use log::LevelFilter::*;
        let levels = [Off, Error, Warn, Info, Debug, Trace];

        for level_filter in levels.iter().rev() {
            let level = if let Some(level) = level_filter.to_level() {
                level
            } else {
                continue;
            };
            let meta = log::Metadata::builder().level(level).build();
            if self.enabled(&meta) {
                log::set_max_level(*level_filter);
                break;
            }
        }

        log::set_boxed_logger(Box::new(self))
    }
    pub fn multi(&self) -> MultiProgress {
        self.bar.clone()
    }
}

impl Log for LogWrapper {
    fn enabled(&self, metadata: &log::Metadata) -> bool {
        self.log.enabled(metadata)
    }

    fn log(&self, record: &log::Record) {
        if self.log.enabled(record.metadata()) {
            self.bar.suspend(|| self.log.log(record))
        }
    }

    fn flush(&self) {
        self.log.flush()
    }
}

pub fn init_logger(
    debug_logs: bool,
    log_dir: Option<&str>,
    logger_type: &LoggerType,
) -> Result<(MultiProgress, ReconfigurationHandle), FlexiLoggerError> {
    let logger_formatter = match &logger_type {
        LoggerType::Deploy => logger_formatter_deploy,
        LoggerType::Activate => logger_formatter_activate,
        LoggerType::Wait => logger_formatter_wait,
        LoggerType::Revoke => logger_formatter_revoke,
    };

    let (logger, handle) = if let Some(log_dir) = log_dir {
        let mut logger = Logger::with_env_or_str("debug")
            .log_to_file()
            .format_for_stderr(logger_formatter)
            .set_palette("196;208;51;7;8".to_string())
            .directory(log_dir)
            .duplicate_to_stderr(match debug_logs {
                true => Duplicate::Debug,
                false => Duplicate::Info,
            })
            .print_message();

        match logger_type {
            LoggerType::Activate => logger = logger.discriminant("activate"),
            LoggerType::Wait => logger = logger.discriminant("wait"),
            LoggerType::Revoke => logger = logger.discriminant("revoke"),
            LoggerType::Deploy => (),
        }

        logger.build()?
    } else {
        Logger::with_env_or_str(match debug_logs {
            true => "debug",
            false => "info",
        })
        .log_target(LogTarget::StdErr)
        .format(logger_formatter)
        .set_palette("196;208;51;7;8".to_string())
        .build()?
    };

    let multi = MultiProgress::new();
    LogWrapper::new(multi.clone(), logger).try_init().unwrap();

    Ok((multi, handle))
}

pub mod cli;
pub mod command;
pub mod data;
pub mod deploy;
pub mod push;

#[derive(Debug, Clone)]
pub struct CmdOverrides {
    pub ssh_user: Option<String>,
    pub profile_user: Option<String>,
    pub ssh_opts: Option<String>,
    pub groups: Option<Vec<String>>,
    pub fast_connection: Option<bool>,
    pub auto_rollback: Option<bool>,
    pub hostname: Option<String>,
    pub magic_rollback: Option<bool>,
    pub temp_path: Option<PathBuf>,
    pub confirm_timeout: Option<u16>,
    pub activation_timeout: Option<u16>,
    pub sudo: Option<String>,
    pub interactive_sudo: Option<bool>,
    pub dry_activate: bool,
    pub remote_build: bool,
}

#[derive(PartialEq, Debug)]
pub struct DeployFlake<'a> {
    pub repo: &'a str,
    pub node: Option<String>,
    pub profile: Option<String>,
}

#[derive(Error, Debug)]
pub enum ParseFlakeError {
    #[error("The given path was too long, did you mean to put something in quotes?")]
    PathTooLong,
    #[error("Unrecognized node or token encountered")]
    Unrecognized,
}

fn parse_fragment(fragment: &str) -> Result<(Option<String>, Option<String>), ParseFlakeError> {
    let mut node: Option<String> = None;
    let mut profile: Option<String> = None;

    let ast = rnix::parse(fragment);

    let first_child = match ast.root().node().first_child() {
        Some(x) => x,
        None => return Ok((None, None)),
    };

    let mut node_over = false;

    for entry in first_child.children_with_tokens() {
        let x: Option<String> = match (entry.kind(), node_over) {
            (TOKEN_DOT, false) => {
                node_over = true;
                None
            }
            (TOKEN_DOT, true) => {
                return Err(ParseFlakeError::PathTooLong);
            }
            (NODE_IDENT, _) => Some(
                entry
                    .into_node()
                    .ok_or(ParseFlakeError::Unrecognized)?
                    .text()
                    .to_string(),
            ),
            (TOKEN_IDENT, _) => Some(
                entry
                    .into_token()
                    .ok_or(ParseFlakeError::Unrecognized)?
                    .text()
                    .to_string(),
            ),
            (NODE_STRING, _) => {
                let c = entry
                    .into_node()
                    .ok_or(ParseFlakeError::Unrecognized)?
                    .children_with_tokens()
                    .nth(1)
                    .ok_or(ParseFlakeError::Unrecognized)?;

                Some(
                    c.into_token()
                        .ok_or(ParseFlakeError::Unrecognized)?
                        .text()
                        .to_string(),
                )
            }
            _ => return Err(ParseFlakeError::Unrecognized),
        };

        if !node_over {
            node = x;
        } else {
            profile = x;
        }
    }

    Ok((node, profile))
}

pub fn parse_flake(flake: &str) -> Result<DeployFlake<'_>, ParseFlakeError> {
    let flake_fragment_start = flake.find('#');
    let (repo, maybe_fragment) = match flake_fragment_start {
        Some(s) => (&flake[..s], Some(&flake[s + 1..])),
        None => (flake, None),
    };

    let mut node: Option<String> = None;
    let mut profile: Option<String> = None;

    if let Some(fragment) = maybe_fragment {
        (node, profile) = parse_fragment(fragment)?;
    }

    Ok(DeployFlake {
        repo,
        node,
        profile,
    })
}

#[test]
fn test_parse_flake() {
    assert_eq!(
        parse_flake("../deploy/examples/system").unwrap(),
        DeployFlake {
            repo: "../deploy/examples/system",
            node: None,
            profile: None,
        }
    );

    assert_eq!(
        parse_flake("../deploy/examples/system#").unwrap(),
        DeployFlake {
            repo: "../deploy/examples/system",
            node: None,
            profile: None,
        }
    );

    assert_eq!(
        parse_flake("../deploy/examples/system#computer.\"something.nix\"").unwrap(),
        DeployFlake {
            repo: "../deploy/examples/system",
            node: Some("computer".to_string()),
            profile: Some("something.nix".to_string()),
        }
    );

    assert_eq!(
        parse_flake("../deploy/examples/system#\"example.com\".system").unwrap(),
        DeployFlake {
            repo: "../deploy/examples/system",
            node: Some("example.com".to_string()),
            profile: Some("system".to_string()),
        }
    );

    assert_eq!(
        parse_flake("../deploy/examples/system#example").unwrap(),
        DeployFlake {
            repo: "../deploy/examples/system",
            node: Some("example".to_string()),
            profile: None
        }
    );

    assert_eq!(
        parse_flake("../deploy/examples/system#example.system").unwrap(),
        DeployFlake {
            repo: "../deploy/examples/system",
            node: Some("example".to_string()),
            profile: Some("system".to_string())
        }
    );

    assert_eq!(
        parse_flake("../deploy/examples/system").unwrap(),
        DeployFlake {
            repo: "../deploy/examples/system",
            node: None,
            profile: None,
        }
    );
}

pub fn parse_file<'a>(
    file: &'a str,
    attribute: &'a str,
) -> Result<DeployFlake<'a>, ParseFlakeError> {
    let (node, profile) = parse_fragment(attribute)?;

    Ok(DeployFlake {
        repo: file,
        node,
        profile,
    })
}

#[derive(Debug, Clone)]
pub struct DeployData {
    pub node_name: String,
    pub node: data::Node,
    pub profile_name: String,
    pub profile: data::Profile,

    pub cmd_overrides: CmdOverrides,

    pub merged_settings: data::GenericSettings,

    pub debug_logs: bool,
    pub log_dir: Option<String>,

    pub progressbar: Option<indicatif::ProgressBar>,
}

#[derive(Debug, Clone)]
pub struct DeployDefs {
    pub ssh_user: String,
    pub profile_user: String,
    pub sudo: Option<String>,
    pub sudo_password: Option<String>,
}
enum ProfileInfo {
    ProfilePath {
        profile_path: String,
    },
    ProfileUserAndName {
        profile_user: String,
        profile_name: String,
    },
}

#[derive(Error, Debug)]
pub enum DeployDataDefsError {
    #[error("Neither `user` nor `sshUser` are set for profile {0} of node {1}")]
    NoProfileUser(String, String),
}

impl DeployData {
    pub fn defs(&self) -> Result<DeployDefs, DeployDataDefsError> {
        let ssh_user = match self.merged_settings.ssh_user {
            Some(ref u) => u.clone(),
            None => whoami::username(),
        };

        let profile_user = self.get_profile_user()?;

        let sudo: Option<String> = match self.merged_settings.user {
            Some(ref user) if user != &ssh_user => Some(format!("{} {}", self.get_sudo(), user)),
            _ => None,
        };

        Ok(DeployDefs {
            ssh_user,
            profile_user,
            sudo,
            sudo_password: None,
        })
    }

    fn get_profile_user(&self) -> Result<String, DeployDataDefsError> {
        let profile_user = match self.merged_settings.user {
            Some(ref x) => x.clone(),
            None => match self.merged_settings.ssh_user {
                Some(ref x) => x.clone(),
                None => {
                    return Err(DeployDataDefsError::NoProfileUser(
                        self.profile_name.to_owned(),
                        self.node_name.to_owned(),
                    ))
                }
            },
        };
        Ok(profile_user)
    }

    fn get_sudo(&self) -> String {
        match self.merged_settings.sudo {
            Some(ref x) => x.clone(),
            None => "sudo -u".to_string(),
        }
    }

    fn get_profile_info(&self) -> Result<ProfileInfo, DeployDataDefsError> {
        match self.profile.profile_settings.profile_path {
            Some(ref profile_path) => Ok(ProfileInfo::ProfilePath {
                profile_path: profile_path.to_string(),
            }),
            None => {
                let profile_user = self.get_profile_user()?;
                Ok(ProfileInfo::ProfileUserAndName {
                    profile_user,
                    profile_name: self.profile_name.to_string(),
                })
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub fn make_deploy_data(
    top_settings: &data::GenericSettings,
    node: &data::Node,
    node_name: String,
    profile: &data::Profile,
    profile_name: String,
    cmd_overrides: &CmdOverrides,
    debug_logs: bool,
    log_dir: Option<String>,
) -> DeployData {
    let mut merged_settings = profile.generic_settings.clone();
    merged_settings.merge(node.generic_settings.clone());
    merged_settings.merge(top_settings.clone());

    // build all machines remotely when the command line flag is set
    if cmd_overrides.remote_build {
        merged_settings.remote_build = Some(cmd_overrides.remote_build);
    }
    if cmd_overrides.ssh_user.is_some() {
        merged_settings.ssh_user = cmd_overrides.ssh_user.clone();
    }
    if cmd_overrides.profile_user.is_some() {
        merged_settings.user = cmd_overrides.profile_user.clone();
    }
    if let Some(ref ssh_opts) = cmd_overrides.ssh_opts {
        merged_settings.ssh_opts = ssh_opts.split(' ').map(|x| x.to_owned()).collect();
    }
    if let Some(fast_connection) = cmd_overrides.fast_connection {
        merged_settings.fast_connection = Some(fast_connection);
    }
    if let Some(auto_rollback) = cmd_overrides.auto_rollback {
        merged_settings.auto_rollback = Some(auto_rollback);
    }
    if let Some(magic_rollback) = cmd_overrides.magic_rollback {
        merged_settings.magic_rollback = Some(magic_rollback);
    }
    if let Some(confirm_timeout) = cmd_overrides.confirm_timeout {
        merged_settings.confirm_timeout = Some(confirm_timeout);
    }
    if let Some(activation_timeout) = cmd_overrides.activation_timeout {
        merged_settings.activation_timeout = Some(activation_timeout);
    }
    if let Some(interactive_sudo) = cmd_overrides.interactive_sudo {
        merged_settings.interactive_sudo = Some(interactive_sudo);
    }

    DeployData {
        node_name,
        node: node.clone(),
        profile_name,
        profile: profile.clone(),
        cmd_overrides: cmd_overrides.clone(),
        merged_settings,
        debug_logs,
        log_dir,
        progressbar: None,
    }
}
