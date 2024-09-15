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
use jsonrpsee::http_client::HttpClient;
use jsonrpsee::core::client::{BatchResponse, ClientT, Error};
use jsonrpsee::core::params::BatchRequestBuilder;
use crate::hive::jsonrpc::request_params::GetBlockParams;
use crate::hive::jsonrpc::responses::GetBlockResponse;

pub async fn get_block(client: &HttpClient, params: GetBlockParams<'_>) -> Result<GetBlockResponse, Error> {
    client.request("block_api.get_block", params).await
}

pub fn build_get_block_batch_params(
    params: GetBlockParams,
    batch_request_builder: &mut BatchRequestBuilder,
) -> Result<(), serde_json::Error> {
    batch_request_builder.insert("block_api.get_block", params)
}

pub async fn get_block_batch(
    client: & HttpClient,
    batch_request_builder: BatchRequestBuilder<'static>,
) -> Result<BatchResponse<'static, GetBlockResponse>, Error> {
    client.batch_request(batch_request_builder).await
}