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
use std::sync::Arc;
use std::time::Duration;
use chrono::{DateTime, TimeDelta, Utc};
use color_eyre::{Report, Result};
use jsonrpsee::core::ClientError::{ParseError, RestartNeeded, Transport};
use jsonrpsee::core::params::BatchRequestBuilder;
use podping_schemas::org::podcastindex::podping::podping_json::Podping;
use tracing::{error, trace, warn};
use tokio::sync::broadcast::Sender;
use tokio::time::sleep;
use regex::Regex;
use tokio::sync::Mutex;
use crate::hive::jsonrpc::{block_api, condenser_api};
use crate::hive::jsonrpc::client::{JsonRpcClient, JsonRpcClientImpl};
use crate::hive::jsonrpc::request_params::GetBlockParams;
use crate::hive::jsonrpc::responses::{GetBlockResponse, GetDynamicGlobalPropertiesResponse};


#[derive(Debug, Clone)]
pub(crate) struct HiveBlockWithNum {
    pub(crate) block_num: u64,
    pub(crate) timestamp: DateTime<Utc>,
    pub(crate) transactions: Vec<HiveTransactionWithTxId>,
}

#[derive(Debug, Clone)]
pub(crate) struct HiveTransactionWithTxId {
    pub(crate) tx_id: String,
    pub(crate) podpings: Vec<Podping>,
}

pub(crate) async fn get_dynamic_global_properties(
    json_rpc_client: Arc<Mutex<JsonRpcClientImpl>>
) -> Result<GetDynamicGlobalPropertiesResponse, Report> {
    let jpc = json_rpc_client.lock().await;
    let client = jpc.get_client();

    let response: Result<GetDynamicGlobalPropertiesResponse, _> = condenser_api::get_dynamic_global_properties(&client).await;
    trace!("condenser_api::get_dynamic_global_properties response: {:?}", response);

    Ok(response?)
}

pub fn block_response_to_hive_block(block_num: u64, id_regex: &Regex, response: GetBlockResponse) -> HiveBlockWithNum {
    HiveBlockWithNum {
        block_num,
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
                                return None;
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
    }
}

async fn send_block<S: Send, T: ToOwned<Owned=S>>(tx: &Sender<S>, block: T) {
    loop {
        match tx.send(block.to_owned()) {
            Ok(_) => break,
            Err(e) => {
                warn!("Scanner send error {}", e);
                sleep(Duration::from_millis(100)).await;
            }
        }
    }
}

struct BlockRangeChunks {
    start_block: u64,
    end_block: u64,
    batch_size: u64,
}

impl BlockRangeChunks {
    pub fn iter(&self) -> BlockRangeChunksIterator {
        BlockRangeChunksIterator {
            chunks: self,
            next_start: self.start_block,
        }
    }
}

struct BlockRangeChunksIterator<'a> {
    chunks: &'a BlockRangeChunks,
    next_start: u64,
}

impl Iterator for BlockRangeChunksIterator<'_> {
    type Item = Vec<u64>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.next_start > self.chunks.end_block {
            return None;
        }

        let range_start = self.next_start;

        let range_end = if range_start + self.chunks.batch_size <= self.chunks.end_block {
            range_start + self.chunks.batch_size
        } else {
            self.chunks.end_block + 1
        };

        self.next_start = range_end;

        Some((range_start..range_end).collect())
    }
}

pub async fn catchup_chain(
    start_block: u64,
    end_block: u64,
    tx: Sender<Vec<HiveBlockWithNum>>,
    json_rpc_client: Arc<Mutex<JsonRpcClientImpl>>,
) -> Result<(), Report> {
    let mut jpc = json_rpc_client.lock().await;
    let mut client = jpc.get_client();

    let id_regex: Regex = Regex::new(r"^pp_(.*)_(.*)|podping$")?;

    let chunks = BlockRangeChunks {
        start_block,
        end_block,
        batch_size: 100,
    };

    for chunk in chunks.iter() {
        let mut batch_request_builder = BatchRequestBuilder::new();

        for block_num in &chunk {
            let params = GetBlockParams {
                block_num: &block_num
            };

            block_api::build_get_block_batch_params(params, &mut batch_request_builder)
                .expect("Error building batch request");
        }

        let batch_response = block_api::get_block_batch(&client, batch_request_builder).await;
        trace!("block_api::get_block batch response: {:?}", batch_response);

        match batch_response {
            Ok(batch_response) => {
                let responses_with_block_num = chunk.into_iter().zip(batch_response);
                let blocks = responses_with_block_num.map(|(block_num, entry)| {
                    let response = entry.unwrap();
                    block_response_to_hive_block(block_num, &id_regex, response)
                }).collect::<Vec<_>>();

                send_block(&tx, blocks).await;
            }
            Err(ParseError(e)) => {
                warn!("Parse error {}", e);
                jpc.rotate_node()?;
                client = jpc.get_client();
                warn!("Retrying block_chunk")
            }
            Err(RestartNeeded(e)) => {
                warn!("{:#?}", e);
                jpc.rotate_node()?;
                client = jpc.get_client();
                warn!("Retrying block_chunk")
            }
            Err(Transport(e)) => {
                warn!("{:#?}", e);
                jpc.rotate_node()?;
                client = jpc.get_client();
                warn!("Retrying block_chunk")
            }
            Err(e) => {
                // Rather brute force error handling
                // the hyper http client seems to have issues with http2 streams closing
                // https://github.com/hyperium/hyper/issues/2500
                // TODO: There's probably a better way to handle it, I just haven't spent the time
                error!("{:#?}", e);
                jpc.rotate_node()?;
                client = jpc.get_client();
            }
        };
    }

    Ok(())
}

pub async fn scan_chain(
    start_block: u64,
    tx: Sender<HiveBlockWithNum>,
    json_rpc_client: Arc<Mutex<JsonRpcClientImpl>>,
) -> Result<(), Report> {
    let mut jpc = json_rpc_client.lock().await;
    let mut client = jpc.get_client();

    let mut block_num = start_block;

    let block_duration = TimeDelta::seconds(3);
    let id_regex: Regex = Regex::new(r"^pp_(.*)_(.*)|podping$")?;

    loop {
        let start_time = Utc::now();

        let params = GetBlockParams {
            block_num: &block_num
        };

        let response: Result<GetBlockResponse, _> = block_api::get_block(&client, params).await;
        trace!("block_api::get_block response: {:?}", response);

        match response {
            Ok(response) => {
                let block = block_response_to_hive_block(block_num, &id_regex, response);

                let block_timestamp = block.timestamp.clone();

                send_block(&tx, block).await;

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
                jpc.rotate_node()?;
                client = jpc.get_client();
                warn!("Retrying block {}", block_num)
            }
            Err(RestartNeeded(e)) => {
                warn!("{:#?}", e);
                jpc.rotate_node()?;
                client = jpc.get_client();
                warn!("Retrying block {}", block_num)
            }
            Err(Transport(e)) => {
                warn!("{:#?}", e);
                jpc.rotate_node()?;
                client = jpc.get_client();
                warn!("Retrying block {}", block_num)
            }
            Err(e) => {
                // Rather brute force error handling
                // the hyper http client seems to have issues with http2 streams closing
                // https://github.com/hyperium/hyper/issues/2500
                // TODO: There's probably a better way to handle it, I just haven't spent the time
                error!("{:#?}", e);
                jpc.rotate_node()?;
                client = jpc.get_client();
            }
        };
    }
}