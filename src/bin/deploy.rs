// SPDX-FileCopyrightText: 2020 Serokell <https://serokell.io/>
// SPDX-FileCopyrightText: 2021 Yannik Sander <contact@ysndr.de>
//
// SPDX-License-Identifier: MPL-2.0

use deploy::cli;
use log::error;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    match cli::run().await {
        Ok(()) => (),
        Err(err) => {
            error!("{}", err);
            std::process::exit(1);
        }
    }

    Ok(())
}
