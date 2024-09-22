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
use chrono::{DateTime, Utc};
use color_eyre::Report;
use tokio::sync::Mutex;
use tokio::task::JoinSet;
use tracing::info;
use crate::config::Settings;
use crate::hive::jsonrpc::client::JsonRpcClient;
use crate::hive::jsonrpc::responses::GetDynamicGlobalPropertiesResponse;
use crate::hive::scanner;
use crate::hive::scanner::HiveBlockWithNum;
use crate::writer::writer::Writer;


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
    writer: Arc<Mutex<impl Writer>>,
    dynamic_global_properties: &GetDynamicGlobalPropertiesResponse,
) -> Result<u64, Report> {
    let last_updated_block = writer.lock().await.get_last_block().await;

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

pub(crate) struct Syncer<'a, J, W> where J: JsonRpcClient + Send, W: Writer + Send {
    json_rpc_client: Arc<Mutex<J>>,
    writer: Arc<Mutex<W>>,
    settings: &'a Settings
}

impl<J: JsonRpcClient + Send + 'static, W: Writer + Send + 'static> Syncer<'_, J, W> {
    pub(crate) fn new(settings: &Settings) -> Result<Syncer<J, W>, Report> {
        Ok(Syncer {
            json_rpc_client: Arc::new(Mutex::new(J::new(settings.scanner.rpc_nodes.clone())?)),
            writer: Arc::new(Mutex::new(W::new(&settings))),
            settings
        })
    }

    pub(crate) async fn start(&self) -> Result<(), Report> {
        let mut dynamic_global_properties = scanner::get_dynamic_global_properties(self.json_rpc_client.clone()).await?;
        let mut start_block = get_start_block(self.settings, self.writer.clone(), &dynamic_global_properties).await?;

        info!("Starting scan at block {}", start_block);

        if start_block < dynamic_global_properties.head_block_number {
            info!("Current block is behind... catching up");

            while start_block < dynamic_global_properties.head_block_number - 2 {
                let (tx, rx) = tokio::sync::broadcast::channel::<Vec<HiveBlockWithNum>>(1);

                let mut catchup_joinset = JoinSet::new();
                catchup_joinset.spawn(
                    scanner::catchup_chain(
                        start_block, dynamic_global_properties.head_block_number, tx, self.json_rpc_client.clone()
                    )
                );

                let writer = self.writer.clone();

                catchup_joinset.spawn(async move {
                    writer.lock().await.start_batch(rx).await
                });

                catchup_joinset.join_all().await;
                start_block = dynamic_global_properties.head_block_number + 1;
                dynamic_global_properties = scanner::get_dynamic_global_properties(self.json_rpc_client.clone()).await?;
            }

            info!("Done catching up! Now at block {}", start_block);
        }

        let mut joinset = JoinSet::new();
        let (tx, rx) = tokio::sync::broadcast::channel::<HiveBlockWithNum>(10);

        let jpc = self.json_rpc_client.clone();
        joinset.spawn(async move {
            scanner::scan_chain(start_block, tx, jpc).await
        });

        let writer = self.writer.clone();
        joinset.spawn(async move {
            writer.lock().await.start(rx).await
        });

        joinset.join_all().await;
        
        Ok(())
    }
}