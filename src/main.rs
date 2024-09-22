/*
 * Copyright (c) 2024 Gates Solutions LLC.
 *
 *      This file is part of podpingd.
 *
 *     podpingd is free software: you can redistribute it and/or modify it under the terms of the GNU Lesser General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
 *
 *     podpingd is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Lesser General Public License for more details.
 *
 *     You should have received a copy of the GNU Lesser General Public License along with podpingd. If not, see <https://www.gnu.org/licenses/>.
 */

mod config;
mod hive;
mod writer;
mod syncer;

use color_eyre::eyre::Result;
use tracing::{info, warn, Level};
use crate::config::CARGO_PKG_VERSION;
use crate::hive::jsonrpc::client::JsonRpcClientImpl;
use crate::syncer::Syncer;
use crate::writer::console_writer::ConsoleWriter;
use crate::writer::disk_writer::DiskWriter;
use crate::writer::writer::Writer;
// for historical purposes
//const FIRST_PODPING_BLOCK: u64 = 53_691_004;



#[tokio::main]
async fn main() -> Result<()> {
    color_eyre::install()?;

    let settings = config::load_config();

    let log_level = match settings.debug {
        false => Level::INFO,
        true => Level::DEBUG
    };

    //let log_level = Level::ERROR;

    tracing_subscriber::fmt()
        .event_format(tracing_subscriber::fmt::format())
        .with_max_level(log_level)
        .with_target(false)
        .init();

    // JSON formatting throwing an error with fields from external libraries
    /*tracing_subscriber::fmt()
        .event_format(tracing_subscriber::fmt::format::json().flatten_event(true))
        .with_max_level(log_level)
        .with_target(false)
        .init();*/

    //let span = span!(Level::INFO, "main").entered();

    let version = CARGO_PKG_VERSION.unwrap_or("VERSION_NOT_FOUND");
    info!("{}", format!("Starting podpingd version {}", version));

    match settings.writer.enabled {
        true => {
            info!("Writing podpings to the the local disk.");
            let syncer = Syncer::<JsonRpcClientImpl, DiskWriter>::new(&settings)?;

            syncer.start().await?;
        },
        false => {
            if !settings.writer.disable_persistence_warnings {
                warn!("The persistent writer is disabled in settings!");

                if settings.scanner.start_block.is_some() || settings.scanner.start_datetime.is_some() {
                    warn!("A start block/date is set.  Without persistence, the scan will start at the values *every time*.")
                }
            }
            
            info!("Writing podpings to the console.");
            
            let syncer = Syncer::<JsonRpcClientImpl, ConsoleWriter>::new(&settings)?;
            
            syncer.start().await?;
        }
    }
    

    //span.exit();

    Ok(())
}
