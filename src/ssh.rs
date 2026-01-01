// SPDX-FileCopyrightText: 2024 deploy-rs contributors
//
// SPDX-License-Identifier: MPL-2.0

use log::{debug, info, warn};
use std::collections::HashMap;
use std::env;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use thiserror::Error;
use tokio::process::Command;
use tokio::sync::Mutex;

fn get_runtime_dir() -> PathBuf {
    if let Ok(runtime_dir) = env::var("XDG_RUNTIME_DIR") {
        return PathBuf::from(runtime_dir).join("deploy-rs");
    }

    if let Ok(tmpdir) = env::var("TMPDIR") {
        return PathBuf::from(tmpdir).join("deploy-rs");
    }

    if let Some(home) = dirs::home_dir() {
        return home.join(".cache").join("deploy-rs");
    }

    PathBuf::from("/tmp").join("deploy-rs")
}

#[derive(Error, Debug)]
pub enum SshError {
    #[error("Failed to create control path directory: {0}")]
    CreateControlDir(std::io::Error),
    #[error("Failed to spawn SSH master: {0}")]
    SpawnMaster(std::io::Error),
    #[error("SSH master exited with error: {0:?}")]
    MasterFailed(Option<i32>),
    #[error("Failed to close SSH master: {0}")]
    CloseMaster(std::io::Error),
}

pub struct SshControlMaster {
    control_path: PathBuf,
    hostname: String,
    ssh_user: Option<String>,
    ssh_opts: Vec<String>,
}

impl SshControlMaster {
    pub fn new(
        hostname: &str,
        ssh_user: Option<&str>,
        ssh_opts: &[String],
        temp_path: &Path,
    ) -> Self {
        let control_path = temp_path.join(format!("deploy-rs-ssh-{}", hostname));

        Self {
            control_path,
            hostname: hostname.to_string(),
            ssh_user: ssh_user.map(|s| s.to_string()),
            ssh_opts: ssh_opts.to_vec(),
        }
    }

    fn ssh_addr(&self) -> String {
        match &self.ssh_user {
            Some(user) => format!("{}@{}", user, self.hostname),
            None => self.hostname.clone(),
        }
    }

    pub fn control_path(&self) -> &Path {
        &self.control_path
    }

    pub fn control_opts(&self) -> Vec<String> {
        vec![
            "-o".to_string(),
            format!("ControlPath={}", self.control_path.display()),
        ]
    }

    pub async fn start(&self) -> Result<(), SshError> {
        if let Some(parent) = self.control_path.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(SshError::CreateControlDir)?;
        }

        let ssh_addr = self.ssh_addr();
        info!("Establishing SSH control master to {}", ssh_addr);

        let mut cmd = Command::new("ssh");
        cmd.arg("-o")
            .arg("ControlMaster=yes")
            .arg("-o")
            .arg(format!("ControlPath={}", self.control_path.display()))
            .arg("-o")
            .arg("ControlPersist=yes")
            .arg("-N")
            .arg("-f");

        for opt in &self.ssh_opts {
            cmd.arg(opt);
        }

        cmd.arg(&ssh_addr);

        debug!("SSH master command: {:?}", cmd);

        let status = cmd.status().await.map_err(SshError::SpawnMaster)?;

        if !status.success() {
            return Err(SshError::MasterFailed(status.code()));
        }

        debug!(
            "SSH control master established at {}",
            self.control_path.display()
        );
        Ok(())
    }

    pub async fn stop(&self) -> Result<(), SshError> {
        if !self.control_path.exists() {
            return Ok(());
        }

        let ssh_addr = self.ssh_addr();
        debug!("Closing SSH control master to {}", ssh_addr);

        let mut cmd = Command::new("ssh");
        cmd.arg("-o")
            .arg(format!("ControlPath={}", self.control_path.display()))
            .arg("-O")
            .arg("exit")
            .arg(&ssh_addr);

        let _ = cmd.status().await;
        Ok(())
    }
}

impl Drop for SshControlMaster {
    fn drop(&mut self) {
        if self.control_path.exists() {
            let _ = std::process::Command::new("ssh")
                .arg("-o")
                .arg(format!("ControlPath={}", self.control_path.display()))
                .arg("-O")
                .arg("exit")
                .arg(&self.ssh_addr())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn();
        }
    }
}

pub struct SshMultiplexer {
    masters: Arc<Mutex<HashMap<String, Arc<SshControlMaster>>>>,
    socket_dir: PathBuf,
}

impl SshMultiplexer {
    pub fn new() -> Self {
        Self {
            masters: Arc::new(Mutex::new(HashMap::new())),
            socket_dir: get_runtime_dir(),
        }
    }

    pub async fn get_or_create(
        &self,
        hostname: &str,
        ssh_user: Option<&str>,
        ssh_opts: &[String],
    ) -> Result<Arc<SshControlMaster>, SshError> {
        let key = format!(
            "{}@{}",
            ssh_user.unwrap_or(""),
            hostname
        );

        let mut masters = self.masters.lock().await;

        if let Some(master) = masters.get(&key) {
            return Ok(Arc::clone(master));
        }

        let master = SshControlMaster::new(hostname, ssh_user, ssh_opts, &self.socket_dir);
        master.start().await?;

        let master = Arc::new(master);
        masters.insert(key, Arc::clone(&master));

        Ok(master)
    }

    pub async fn close_all(&self) {
        let mut masters = self.masters.lock().await;

        for (key, master) in masters.drain() {
            if let Err(e) = master.stop().await {
                warn!("Failed to close SSH master for {}: {}", key, e);
            }
        }
    }
}
