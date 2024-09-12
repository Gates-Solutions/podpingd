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
use color_eyre::Result;
use jsonrpsee::core::traits::ToRpcParams;
use serde::Serialize;
use serde_json::value::RawValue;

pub(crate) struct EmptyParams;

impl ToRpcParams for EmptyParams {
    fn to_rpc_params(self) -> Result<Option<Box<RawValue>>, serde_json::Error> {
        Ok(Some(serde_json::value::to_raw_value(&Vec::<u64>::new())?))
    }
}

#[derive(Serialize, Debug)]
pub(crate) struct GetBlockParams {
    pub(crate) block_num: u64
}

impl ToRpcParams for GetBlockParams {
    fn to_rpc_params(self) -> Result<Option<Box<RawValue>>, serde_json::Error> {
        Ok(Some(serde_json::value::to_raw_value(&self)?))
    }
}