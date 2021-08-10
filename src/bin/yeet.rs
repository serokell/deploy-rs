// SPDX-FileCopyrightText: 2020 Serokell <https://serokell.io/>
// SPDX-FileCopyrightText: 2021 Yannik Sander <contact@ysndr.de>
//
// SPDX-License-Identifier: MPL-2.0

use log::error;
use yeet::cli;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    match cli::run(None).await {
        Ok(()) => (),
        Err(err) => {
            error!("{}", err);
            std::process::exit(1);
        }
    }

    Ok(())
}
