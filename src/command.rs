use std::ffi::OsStr;
use std::fmt;
use std::fmt::Debug;
use std::future::Future;
use thiserror::Error;
use tokio::process::Command as TokioCommand;

pub trait HasCommandError {
    fn title() -> String;
}

#[derive(Error, Debug)]
pub enum CommandError<T: fmt::Debug + fmt::Display + HasCommandError> {
    RunError(std::io::Error),
    Exit(Option<i32>, String),
    OtherError(T),
}

impl<T: fmt::Debug + fmt::Display + HasCommandError> fmt::Display for CommandError<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CommandError::RunError(err) => write!(
                f,
                "Failed to run {} command: {}",
                T::title(),
                err,
            ),
            CommandError::Exit(exit_code, cmd) => write!(
                f,
                "{} command resulted in a bad exit code: {:?}. The failed command is provided below:\n{}",
                T::title(),
                exit_code,
                cmd,
            ),
            CommandError::OtherError(err) => write!(f, "{}", err),
        }
    }
}

/// A wrapper over `tokio::process::Command` to provide the `run` method commonly used by `deploy`.
#[derive(Debug)]
pub struct Command {
    pub command: TokioCommand,
}

impl Command {
    pub fn new<S: AsRef<OsStr>>(program: S) -> Command {
        Command {
            command: TokioCommand::new(program),
        }
    }

    pub fn arg<S: AsRef<OsStr>>(&mut self, arg: S) -> &mut Command {
        self.command.arg(arg);
        self
    }

    pub fn args<I, S>(&mut self, args: I) -> &mut Command
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        self.command.args(args);
        self
    }

    pub fn env<K, V>(&mut self, key: K, val: V) -> &mut Command
    where
        K: AsRef<OsStr>,
        V: AsRef<OsStr>,
    {
        self.command.env(key, val);
        self
    }

    pub fn output(&mut self) -> impl Future<Output = tokio::io::Result<std::process::Output>> {
        self.command.output()
    }

    pub fn current_dir<P: AsRef<std::path::Path>>(&mut self, dir: P) -> &mut Command {
        self.command.current_dir(dir);
        self
    }

    pub fn stdin<T: Into<std::process::Stdio>>(&mut self, cfg: T) -> &mut Command {
        self.command.stdin(cfg);
        self
    }

    pub fn stdout<T: Into<std::process::Stdio>>(&mut self, cfg: T) -> &mut Command {
        self.command.stdout(cfg);
        self
    }

    pub fn stderr<T: Into<std::process::Stdio>>(&mut self, cfg: T) -> &mut Command {
        self.command.stderr(cfg);
        self
    }

    pub fn spawn(&mut self) -> std::io::Result<tokio::process::Child> {
        self.command.spawn()
    }

    pub fn status(&mut self) -> impl Future<Output = tokio::io::Result<std::process::ExitStatus>> {
        self.command.status()
    }

    pub async fn run<T: fmt::Debug + fmt::Display + HasCommandError>(
        &mut self,
    ) -> Result<std::process::Output, CommandError<T>> {
        let output = self
            .command
            .output()
            .await
            .map_err(CommandError::RunError)?;
        match output.status.code() {
            Some(0) => Ok(output),
            exit_code => Err(CommandError::Exit(exit_code, format!("{:?}", self.command))),
        }
    }
}

impl fmt::Display for Command {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.command.fmt(f)
    }
}
