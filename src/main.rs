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

use chrono::{Datelike, Timelike};
use color_eyre::eyre::Result;
use color_eyre::Report;
use podping_schemas::org::podcastindex::podping::podping_json::Podping;
use tokio::sync::broadcast::error::RecvError;
use tokio::sync::broadcast::Receiver;
use tracing::{debug, info, warn, Level};
use hive::scanner;
use crate::config::CARGO_PKG_VERSION;
use crate::hive::scanner::HiveBlockWithNum;


const FIRST_PODPING_BLOCK: u64 = 53_691_004;

async fn podping_disk_writer(mut rx: Receiver<HiveBlockWithNum>) -> Result<(), Report> {
    // TODO: Make output directory configurable
    let last_block_file = "./temp_output/last_updated_block";

    loop {
        let result = rx.recv().await;

        let block = match result {
            Ok(block) => Some(block),
            Err(RecvError::Lagged(e)) => {
                warn!("Disk writer is lagging: {}", e);

                None
            }
            Err(RecvError::Closed) => {
                panic!("Disk writer channel closed");
            }
        };
        match block {
            Some(block) => {
                if block.transactions.is_empty() {
                    info!("No Podpings for block {}", block.block_num);
                } else {
                    let current_block_dir = format!(
                        "./temp_output/{}/{}/{}/{}/{}/{}",
                        block.timestamp.year(),
                        block.timestamp.month(),
                        block.timestamp.day(),
                        block.timestamp.hour(),
                        block.timestamp.minute(),
                        block.timestamp.second(),
                    );

                    std::fs::create_dir_all(&current_block_dir).unwrap_or(());

                    for tx in block.transactions {
                        for (i, podping) in tx.podpings.iter().enumerate() {
                            let json = serde_json::to_string(&podping)?;
                            info!("block: {}, tx: {}, podping: {}", block.block_num, tx.tx_id, json);

                            let file = match podping {
                                Podping::V0(_)
                                | Podping::V02(_)
                                | Podping::V03(_)
                                | Podping::V10(_) => format!("{}/{}_{}.json", current_block_dir, tx.tx_id, i),
                                Podping::V11(pp) => format!(
                                    "{}/{}_{}_{}.json",
                                    current_block_dir,
                                    tx.tx_id,
                                    pp.session_id.to_string(),
                                    pp.timestamp_ns.to_string()
                                ),
                            };
                            info!("Writing podping to file: {}", file);
                            std::fs::write(file, json)?;
                        }
                    }
                }

                std::fs::write(last_block_file, block.block_num.to_string())?;
            }
            None => {}
        }
    };
}

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

    let last_block_file = "./temp_output/last_updated_block";
    let last_updated_block = match std::fs::read_to_string(last_block_file) {
        Ok(s) => {
            info!("Last updated block: {}", s);
            Some(s.parse::<u64>()?)
        }
        _ => None
    };

    // TODO: add custom start time
    let next_block = match last_updated_block {
        Some(last_updated_block) => last_updated_block + 1,
        _ => FIRST_PODPING_BLOCK
    };

    info!("Starting scan at block {}", next_block);

    let (tx, rx) = tokio::sync::broadcast::channel::<HiveBlockWithNum>(1);

    // TODO: Check if next block is in the past and catch up with batch requests
    // Haven't tested batch requests with jsonrpsee yet
    let scanner_handle = tokio::spawn(async move {
        scanner::scan_chain(next_block, tx).await;
    });

    let disk_writer_handle = tokio::spawn(async move {
        podping_disk_writer(rx).await;
    });

    scanner_handle.await?;
    disk_writer_handle.await?;

    //span.exit();

    Ok(())
}

