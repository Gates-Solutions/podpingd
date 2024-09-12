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
use jsonrpsee::core::client::{ClientT, Error};
use crate::hive::jsonrpc::request_params::GetBlockParams;
use crate::hive::jsonrpc::responses::GetBlockResponse;

pub async fn get_block(client: &HttpClient, params: GetBlockParams) -> Result<GetBlockResponse, Error> {
    client.request("block_api.get_block", params).await
}