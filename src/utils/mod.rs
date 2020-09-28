use std::borrow::Cow;
use std::path::PathBuf;

pub mod data;
pub mod deploy;
pub mod push;

macro_rules! good_panic {
    ($($tts:tt)*) => {{
        error!($($tts)*);
        std::process::exit(1);
    }}
}

pub struct DeployData<'a> {
    pub sudo: Option<String>,
    pub ssh_user: Cow<'a, str>,
    pub profile_user: Cow<'a, str>,
    pub profile_path: String,
    pub current_exe: PathBuf,
}

pub async fn make_deploy_data<'a>(
    profile_name: &str,
    node_name: &str,
    merged_settings: &'a data::GenericSettings,
) -> Result<DeployData<'a>, Box<dyn std::error::Error>> {
    let ssh_user: Cow<str> = match &merged_settings.ssh_user {
        Some(u) => u.into(),
        None => whoami::username().into(),
    };

    let profile_user: Cow<str> = match &merged_settings.user {
        Some(x) => x.into(),
        None => match &merged_settings.ssh_user {
            Some(x) => x.into(),
            None => good_panic!(
                "Neither user nor sshUser set for profile `{}` of node `{}`",
                profile_name,
                node_name
            ),
        },
    };

    let profile_path = match &profile_user[..] {
        "root" => format!("/nix/var/nix/profiles/{}", profile_name),
        _ => format!(
            "/nix/var/nix/profiles/per-user/{}/{}",
            profile_user, profile_name
        ),
    };

    let sudo: Option<String> = match merged_settings.user {
        Some(ref user) if user != &ssh_user => Some(format!("sudo -u {}", user)),
        _ => None,
    };

    let current_exe = std::env::current_exe().expect("Expected to find current executable path");

    if !current_exe.starts_with("/nix/store/") {
        good_panic!("The deploy binary must be in the Nix store");
    }

    Ok(DeployData {
        sudo,
        ssh_user,
        profile_user,
        profile_path,
        current_exe,
    })
}
