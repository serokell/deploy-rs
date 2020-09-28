use clap::Clap;

extern crate pretty_env_logger;
#[macro_use]
extern crate log;

#[macro_use]
extern crate serde_derive;

#[macro_use]
mod utils;

/// Activation portion of the simple Rust Nix deploy tool
#[derive(Clap, Debug)]
#[clap(version = "1.0", author = "notgne2 <gen2@gen2.space>")]
struct Opts {
    profile_path: String,
    closure: String,

    /// Command for activating the given profile
    #[clap(long)]
    activate_cmd: Option<String>,

    /// Command for bootstrapping
    #[clap(long)]
    bootstrap_cmd: Option<String>,

    /// Auto rollback if failure
    #[clap(long)]
    auto_rollback: bool,
}


#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    if std::env::var("DEPLOY_LOG").is_err() {
        std::env::set_var("DEPLOY_LOG", "info");
    }

    pretty_env_logger::init_custom_env("DEPLOY_LOG");

    let opts: Opts = Opts::parse();

    utils::activate::activate(
        opts.profile_path,
        opts.closure,
        opts.activate_cmd,
        opts.bootstrap_cmd,
        opts.auto_rollback,
    )
    .await?;

    Ok(())
}