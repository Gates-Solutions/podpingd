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
use std::time::Duration;
use chrono::{DateTime, TimeDelta, Utc};
use color_eyre::{Report, Result};
use jsonrpsee::core::ClientError::{ParseError, RestartNeeded, Transport};
use podping_schemas::org::podcastindex::podping::podping_json::{Podping, PodpingV02, PodpingV03};
use tracing::{debug, error, info, trace, warn};
use jsonrpsee::http_client::HttpClient;
use tokio::sync::broadcast::Sender;
use tokio::time::sleep;
use regex::Regex;
use crate::hive::jsonrpc::{block_api, condenser_api};
use crate::hive::jsonrpc::request_params::GetBlockParams;
use crate::hive::jsonrpc::responses::{GetBlockResponse, GetDynamicGlobalPropertiesResponse, HiveTransaction};


#[derive(Debug, Clone)]
pub(crate) struct HiveBlockWithNum {
    pub(crate) block_num: u64,
    pub(crate) block_id: String,
    pub(crate) timestamp: DateTime<Utc>,
    pub(crate) transactions: Vec<HiveTransactionWithTxId>,
}

#[derive(Debug, Clone)]
pub(crate) struct HiveTransactionWithTxId {
    pub(crate) tx_id: String,
    pub(crate) podpings: Vec<Podping>,
}

pub(crate) async fn get_current_block_num() -> Result<u64, Report> {
    let client = HttpClient::builder().build("https://rpc.podping.org")?;

    let response: Result<GetDynamicGlobalPropertiesResponse, _> = condenser_api::get_dynamic_global_properties(&client).await;
    trace!("condenser_api::get_dynamic_global_properties response: {:?}", response);

    Ok(response?.head_block_number)
}

pub async fn scan_chain(start_block: u64, tx: Sender<HiveBlockWithNum>) -> Result<(), Report> {
    // TODO: Set a configurable RPC server, and ideally a list of RPC servers to rotate
    let mut client = HttpClient::builder().build("https://rpc.podping.org")?;

    let mut block_num = start_block;

    let block_duration = TimeDelta::seconds(3);
    let id_regex: Regex = Regex::new(r"^pp_(.*)_(.*)|podping$")?;

    loop {
        let start_time = Utc::now();

        let params = GetBlockParams {
            block_num
        };

        let response: Result<GetBlockResponse, _> = block_api::get_block(&client, params).await;
        trace!("block_api::get_block response: {:?}", response);

        match response {
            Ok(response) => {
                let block = HiveBlockWithNum {
                    block_num,
                    block_id: response.block.block_id,
                    timestamp: response.block.timestamp,
                    transactions: response.block.transactions.into_iter()
                        .enumerate()
                        .flat_map(|(i, tx)| {
                            Some(HiveTransactionWithTxId {
                                tx_id: response.block.transaction_ids[i].to_string(),
                                podpings: tx.operations.into_iter()
                                    .filter_map(|op| -> Option<Podping> {
                                        // I tried to move this into its own function,
                                        // but failed miserably because I needed a closure
                                        // and probably violated some lifetime thing
                                        //
                                        // Don't judge me.

                                        if op.type_ != "custom_json_operation" {
                                            return None
                                        }

                                        match &op.value {
                                            Some(op_value) => {
                                                match &op_value.id {
                                                    Some(id) => {
                                                        if id_regex.is_match(id) {
                                                            match &op.value {
                                                                Some(op_value) => {
                                                                    match &op_value.json {
                                                                        Some(podping) => {
                                                                            Some(podping.clone())
                                                                        }
                                                                        None => None
                                                                    }
                                                                }
                                                                None => None
                                                            }
                                                        } else {
                                                            None
                                                        }
                                                    }
                                                    None => None
                                                }
                                            }
                                            None => None
                                        }
                                    })
                                    .collect::<Vec<_>>(),
                            })
                        })
                        .filter(|tx| {
                            !tx.podpings.is_empty()
                        }).collect::<Vec<_>>(),
                };
                let block_timestamp = block.timestamp.clone();

                loop {
                    match tx.send(block.to_owned()) {
                        Ok(_) => break,
                        Err(e) => {
                            warn!("Scanner send error {}", e);
                            sleep(Duration::from_millis(100)).await;
                        }
                    }
                }

                block_num += 1;

                let end_time = Utc::now();

                let time_since_block = end_time - block_timestamp;
                if time_since_block < block_duration {
                    let loop_time = end_time - start_time;

                    if loop_time < block_duration {
                        let sleep_duration = block_duration - loop_time;
                        sleep(sleep_duration.to_std()?).await;
                    }
                }
            }
            Err(ParseError(e)) => {
                warn!("Parse error {}", e);
                warn!("Retrying block {}", block_num)
            }
            Err(RestartNeeded(e)) => {
                warn!("{:#?}", e);
                client = HttpClient::builder().build("https://rpc.podping.org")?;
                warn!("Retrying block {}", block_num)
            }
            Err(Transport(e)) => {
                warn!("{:#?}", e);
                warn!("Retrying block {}", block_num)
            }
            Err(e) => {
                // Rather brute force error handling
                // the hyper http client seems to have issues with http2 streams closing
                // https://github.com/hyperium/hyper/issues/2500
                // TODO: There's probably a better way to handle it, I just haven't spent the time
                error!("{:#?}", e);
                client = HttpClient::builder().build("https://rpc.podping.org")?;
            }
        };
    }
}