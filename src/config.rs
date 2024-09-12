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

use config::{Config, File};
use serde::Deserialize;

pub(crate) const CARGO_PKG_VERSION: Option<&'static str> = option_env!("CARGO_PKG_VERSION");


#[derive(Debug, Deserialize)]
#[allow(unused)]
pub struct Settings {
    pub(crate) debug: bool,
}

pub(crate) fn load_config() -> Settings {
    let config = Config::builder()
        .add_source(File::with_name("conf/00-default.toml"))
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