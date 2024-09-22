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
use std::path::Path;
use std::time::Duration;
use chrono::{DateTime, Utc};
use config::{Config, File};
use serde::Deserialize;

pub(crate) const CARGO_PKG_VERSION: Option<&'static str> = option_env!("CARGO_PKG_VERSION");

#[derive(Debug, Deserialize)]
pub struct Scanner {
    pub(crate) rpc_nodes: Vec<String>,
    pub(crate) start_block: Option<u64>,
    pub(crate) start_datetime: Option<DateTime<Utc>>,
}

#[derive(Debug, Deserialize)]
pub enum WriterType {
    Disk,
    ObjectStorage
}

#[derive(Debug, Deserialize)]
pub struct Writer {
    pub(crate) enabled: bool,
    pub(crate) disable_persistence_warnings: bool,
    
    #[serde(rename = "type")]
    pub(crate) type_: Option<WriterType>,

    pub(crate) disk_directory: Option<String>,
    pub(crate) disk_trim_old: Option<bool>,
    #[serde(with = "humantime_serde")]
    pub(crate) disk_trim_keep_duration: Option<Duration>,


    pub(crate) object_storage_base_url: Option<String>,
    pub(crate) object_storage_bucket_name: Option<String>,
    pub(crate) object_storage_access_key: Option<String>,
    pub(crate) object_storage_access_secret: Option<String>,
}

#[derive(Debug, Deserialize)]
#[allow(unused)]
pub struct Settings {
    pub(crate) debug: bool,
    pub(crate) scanner: Scanner,
    pub(crate) writer: Writer,
}



pub(crate) fn load_config() -> Settings {
    let user_config_file_option = option_env!("PODPINGD_CONFIG_FILE");

    let user_config_file = match user_config_file_option {
        Some(file) => {
            if !Path::new(file).exists() {
                panic!("File {} defined by PODPINGD_CONFIG_FILE does not exist", file);
            }
            file
        },
        None => ""
    };

    let config = Config::builder()
        .add_source(File::with_name("conf/00-default.toml"))
        .add_source(File::with_name(user_config_file).required(false))
        .add_source(
            config::Environment::with_prefix("PODPINGD")
                .try_parsing(true)
                .separator("_")
                .list_separator(" "),
        )
        .build()
        .unwrap();

    config.try_deserialize().unwrap()
}