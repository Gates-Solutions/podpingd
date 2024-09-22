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
use color_eyre::Report;
use tokio::sync::broadcast::Receiver;
use tracing::{error, info, warn};
use tokio::sync::broadcast::error::RecvError;
use crate::config::Settings;
use crate::hive::scanner::HiveBlockWithNum;
use crate::writer::writer::Writer;

pub fn console_output_block_transactions(block: HiveBlockWithNum) -> color_eyre::Result<(), Report> {
    if block.transactions.is_empty() {
        info!("No Podpings for block {}", block.block_num);
    } else {
        for tx in &block.transactions {
            for podping in tx.podpings.iter() {

                let json = serde_json::to_string(&podping);

                match json {
                    Ok(json) => {
                        info!("block: {}, tx: {}, podping: {}", block.block_num, tx.tx_id, json);
                    }
                    Err(e) => {
                        error!("Error outputting podping: {}", e);
                    }
                }
            }
        }
    }
    Ok(())
}

pub(crate) struct ConsoleWriter {}

impl Writer for ConsoleWriter {
    fn new(_: &Settings) -> Self
    where
        Self: Sized,
    {
        ConsoleWriter {}
    }

    async fn get_last_block(&self) -> Option<u64> {
        None
    }

    async fn start(&self, mut rx: Receiver<HiveBlockWithNum>) -> color_eyre::Result<(), Report> {
        loop {
            let result = rx.recv().await;

            let block = match result {
                Ok(block) => Some(block),
                Err(RecvError::Lagged(e)) => {
                    warn!("Console writer is lagging: {}", e);

                    None
                }
                Err(RecvError::Closed) => {
                    panic!("Console writer channel closed");
                }
            };
            match block {
                Some(block) => {
                    console_output_block_transactions(block)?;
                }
                None => {}
            }
        };
    }

    async fn start_batch(&self, mut rx: Receiver<Vec<HiveBlockWithNum>>) -> color_eyre::Result<(), Report> {
        loop {
            let result = rx.recv().await;

            let block = match result {
                Ok(block) => Some(block),
                Err(RecvError::Lagged(e)) => {
                    warn!("Console writer is lagging: {}", e);

                    None
                }
                Err(RecvError::Closed) => {
                    break
                }
            };

            match block {
                Some(blocks) => {
                    for block in blocks {
                        console_output_block_transactions(block)?;
                    }
                }
                None => {}
            }
        };

        Ok(())
    }
}