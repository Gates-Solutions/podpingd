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

use std::path::PathBuf;
use std::sync::Arc;
use chrono::{DateTime, Datelike, Timelike, Utc};
use color_eyre::eyre::Result;
use color_eyre::Report;
use podping_schemas::org::podcastindex::podping::podping_json::Podping;
use tokio::sync::broadcast::error::RecvError;
use tokio::sync::broadcast::Receiver;
use tokio::sync::Mutex;
use tokio::task::JoinSet;
use tracing::{error, info, warn, Level};
use hive::scanner;
use crate::config::{Settings, CARGO_PKG_VERSION};
use crate::hive::jsonrpc::client::{JsonRpcClient, JsonRpcClientImpl};
use crate::hive::jsonrpc::responses::GetDynamicGlobalPropertiesResponse;
use crate::hive::scanner::HiveBlockWithNum;

// for historical purposes
//const FIRST_PODPING_BLOCK: u64 = 53_691_004;

const LAST_UPDATED_BLOCK_FILENAME: &str = "last_updated_block";


async fn write_block_transactions(data_dir_path: PathBuf, block: HiveBlockWithNum) -> Result<(), Report> {
    if block.transactions.is_empty() {
        info!("No Podpings for block {}", block.block_num);
    } else {
        let current_block_dir = data_dir_path
            .join(block.timestamp.year().to_string())
            .join(block.timestamp.month().to_string())
            .join(block.timestamp.day().to_string())
            .join(block.timestamp.hour().to_string())
            .join(block.timestamp.minute().to_string())
            .join(block.timestamp.second().to_string());

        let create_dir_future = tokio::fs::create_dir_all(&current_block_dir);

        let mut write_join_set = JoinSet::new();

        for tx in &block.transactions {
            for (i, podping) in tx.podpings.iter().enumerate() {
                let podping_file = match podping {
                    Podping::V0(_)
                    | Podping::V02(_)
                    | Podping::V03(_)
                    | Podping::V10(_) => current_block_dir
                        .join(format!("{}_{}_{}.json", block.block_num, tx.tx_id, i)),
                    Podping::V11(pp) => current_block_dir
                        .join(format!(
                            "{}_{}_{}_{}.json",
                            block.block_num,
                            tx.tx_id,
                            pp.session_id.to_string(),
                            pp.timestamp_ns.to_string())
                        ),
                };

                let json = serde_json::to_string(&podping);

                match json {
                    Ok(json) => {
                        info!("block: {}, tx: {}, podping: {}", block.block_num, tx.tx_id, json);

                        info!("Writing podping to file: {}", podping_file.to_string_lossy());
                        write_join_set.spawn(tokio::fs::write(podping_file, json));
                    }
                    Err(e) => {
                        error!("Error writing podping file {}: {}", podping_file.to_string_lossy(), e);
                    }
                }
            }
        }

        create_dir_future.await?;

        write_join_set.join_all().await;
    }
    Ok(())
}

async fn batch_disk_writer(mut rx: Receiver<Vec<HiveBlockWithNum>>, data_dir_path: PathBuf) -> Result<(), Report> {
    let last_block_file_path = data_dir_path.join(LAST_UPDATED_BLOCK_FILENAME);

    loop {
        let result = rx.recv().await;

        let block = match result {
            Ok(block) => Some(block),
            Err(RecvError::Lagged(e)) => {
                warn!("Disk writer is lagging: {}", e);

                None
            }
            Err(RecvError::Closed) => {
                break
            }
        };

        match block {
            Some(blocks) => {
                let last_block_num = &blocks.last().unwrap().block_num.to_string();
                let mut write_join_set = JoinSet::new();

                for block in blocks {
                    write_join_set.spawn(write_block_transactions(data_dir_path.clone(), block));
                }

                write_join_set.join_all().await;

                tokio::fs::write(&last_block_file_path, last_block_num.to_string()).await?;
            }
            None => {}
        }
    };

    Ok(())
}

async fn podping_disk_writer(mut rx: Receiver<HiveBlockWithNum>, data_dir_path: PathBuf) -> Result<(), Report> {
    let last_block_file_path = data_dir_path.join(LAST_UPDATED_BLOCK_FILENAME);

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
                let block_num = block.block_num.to_owned();
                write_block_transactions(data_dir_path.clone(), block).await?;
                tokio::fs::write(&last_block_file_path, block_num.to_string()).await?
            }
            None => {}
        }
    };
}

async fn get_start_block_from_global_properties(
    start_datetime: Option<DateTime<Utc>>,
    dynamic_global_properties: &GetDynamicGlobalPropertiesResponse,
) -> Result<u64, Report> {
    match start_datetime {
        Some(start_datetime) => {
            if start_datetime > dynamic_global_properties.time {
                panic!("start_datetime {} is in the future!", start_datetime)
            }

            let time_delta = dynamic_global_properties.time - start_datetime;
            let num_blocks_ago = time_delta.num_seconds() / 3;

            Ok(dynamic_global_properties.head_block_number - num_blocks_ago as u64)
        }
        None => Ok(dynamic_global_properties.head_block_number)
    }
}

async fn get_start_block(
    settings: &Settings,
    last_block_file: PathBuf,
    dynamic_global_properties: &GetDynamicGlobalPropertiesResponse,
) -> Result<u64, Report> {
    let last_updated_block = match std::fs::read_to_string(last_block_file) {
        Ok(s) => {
            info!("Last updated block: {}", s);
            Some(s.trim().parse::<u64>()?)
        }
        _ => None
    };

    match last_updated_block {
        Some(last_updated_block) => Ok(last_updated_block + 1),
        None => match settings.scanner.start_block {
            Some(start_block) => Ok(start_block),
            None => get_start_block_from_global_properties(
                settings.scanner.start_datetime, dynamic_global_properties,
            ).await
        }
    }
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

    let data_dir_path = match settings.data_directory.is_empty() {
        true => panic!("Data directory is empty"),
        false => PathBuf::from(settings.data_directory.clone()),
    };

    if !data_dir_path.is_dir() {
        panic!("Data directory {} is not a directory.  Please ensure it exists", data_dir_path.display());
    }

    let last_block_file = data_dir_path.join(LAST_UPDATED_BLOCK_FILENAME);
    let json_rpc_client = Arc::new(Mutex::new(JsonRpcClientImpl::new(settings.scanner.rpc_nodes.clone())?));

    let mut dynamic_global_properties = scanner::get_dynamic_global_properties(json_rpc_client.clone()).await?;
    let mut start_block = get_start_block(&settings, last_block_file, &dynamic_global_properties).await?;

    info!("Starting scan at block {}", start_block);

    if start_block < dynamic_global_properties.head_block_number {
        info!("Current block is behind... catching up");
        while start_block < dynamic_global_properties.head_block_number-10 {
            let (tx, rx) = tokio::sync::broadcast::channel::<Vec<HiveBlockWithNum>>(1);

            let mut catchup_joinset = JoinSet::new();
            catchup_joinset.spawn(
                scanner::catchup_chain(
                    start_block, dynamic_global_properties.head_block_number, tx, json_rpc_client.clone()
                )
            );
            catchup_joinset.spawn(batch_disk_writer(rx, data_dir_path.clone()));

            catchup_joinset.join_all().await;
            start_block = dynamic_global_properties.head_block_number + 1;
            dynamic_global_properties = scanner::get_dynamic_global_properties(json_rpc_client.clone()).await?;
        }

        info!("Done catching up! Now at block {}", start_block);
    }

    let (tx, rx) = tokio::sync::broadcast::channel::<HiveBlockWithNum>(1);

    let scanner_handle = tokio::spawn(async move {
        let _ = scanner::scan_chain(start_block, tx, json_rpc_client.clone()).await;
    });

    let disk_writer_handle = tokio::spawn(async move {
        let _ = podping_disk_writer(rx, data_dir_path.clone()).await;
    });

    scanner_handle.await?;
    disk_writer_handle.await?;

    //span.exit();

    Ok(())
}

