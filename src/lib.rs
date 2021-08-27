// SPDX-FileCopyrightText: 2020 Serokell <https://serokell.io/>
// SPDX-FileCopyrightText: 2020 Andreas Fuchs <asf@boinkor.net>
// SPDX-FileCopyrightText: 2021 Yannik Sander <contact@ysndr.de>
//
// SPDX-License-Identifier: MPL-2.0

use flexi_logger::*;

pub fn make_lock_path(temp_path: &str, closure: &str) -> String {
    let lock_hash =
        &closure["/nix/store/".len()..closure.find('-').unwrap_or_else(|| closure.len())];
    format!("{}/deploy-rs-canary-{}", temp_path, lock_hash)
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

pub fn init_logger(
    debug_logs: bool,
    log_dir: Option<&str>,
    logger_type: &LoggerType,
) -> Result<(), FlexiLoggerError> {
    let logger_formatter = match &logger_type {
        LoggerType::Deploy => logger_formatter_deploy,
        LoggerType::Activate => logger_formatter_activate,
        LoggerType::Wait => logger_formatter_wait,
        LoggerType::Revoke => logger_formatter_revoke,
    };

    if let Some(log_dir) = log_dir {
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

        logger.start()?;
    } else {
        Logger::with_env_or_str(match debug_logs {
            true => "debug",
            false => "info",
        })
        .log_target(LogTarget::StdErr)
        .format(logger_formatter)
        .set_palette("196;208;51;7;8".to_string())
        .start()?;
    }

    Ok(())
}

pub mod cli;
pub mod data;
pub mod deploy;
pub mod flake;
pub mod push;
pub mod settings;
