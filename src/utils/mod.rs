use std::borrow::Cow;
use std::path::PathBuf;

#[macro_export]
macro_rules! good_panic {
    ($($tts:tt)*) => {{
        error!($($tts)*);
        std::process::exit(1);
    }}
}

pub mod data;
pub mod deploy;
pub mod push;

#[derive(PartialEq, Debug)]
pub struct DeployFlake<'a> {
    pub repo: &'a str,
    pub node: Option<&'a str>,
    pub profile: Option<&'a str>,
}

pub fn parse_flake(flake: &str) -> DeployFlake {
    let flake_fragment_start = flake.find('#');
    let (repo, maybe_fragment) = match flake_fragment_start {
        Some(s) => (&flake[..s], Some(&flake[s + 1..])),
        None => (flake, None),
    };

    let (node, profile) = match maybe_fragment {
        Some(fragment) => {
            let fragment_profile_start = fragment.find('.');
            match fragment_profile_start {
                Some(s) => (Some(&fragment[..s]), Some(&fragment[s + 1..])),
                None => (Some(fragment), None),
            }
        }
        None => (None, None),
    };

    DeployFlake {
        repo,
        node,
        profile,
    }
}

#[test]
fn test_parse_flake() {
    assert_eq!(
        parse_flake("../deploy/examples/system#example"),
        DeployFlake {
            repo: "../deploy/examples/system",
            node: Some("example"),
            profile: None
        }
    );

    assert_eq!(
        parse_flake("../deploy/examples/system#example.system"),
        DeployFlake {
            repo: "../deploy/examples/system",
            node: Some("example"),
            profile: Some("system")
        }
    );

    assert_eq!(
        parse_flake("../deploy/examples/system"),
        DeployFlake {
            repo: "../deploy/examples/system",
            node: None,
            profile: None,
        }
    );
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
