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
use serde_with::DefaultOnError;
use serde_with::serde_as;
use serde::Deserialize;
use chrono::{DateTime, Utc};
use podping_schemas::org::podcastindex::podping::podping_json::Podping;

// chrono doesn't appear to support ISO8601 without timezone offsets
// https://github.com/chronotope/chrono/issues/587
// Example from https://serde.rs/custom-date-format.html
pub(crate) mod hive_datetime_format {
    use chrono::{DateTime, NaiveDateTime, Utc};
    use serde::{self, Deserialize, Deserializer, Serializer};

    const FORMAT: &'static str = "%Y-%m-%dT%H:%M:%S";

    pub fn serialize<S>(
        date: &DateTime<Utc>,
        serializer: S,
    ) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let s = format!("{}", date.format(FORMAT));
        serializer.serialize_str(&s)
    }

    pub fn deserialize<'de, D>(
        deserializer: D,
    ) -> Result<DateTime<Utc>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        let dt = NaiveDateTime::parse_from_str(&s, FORMAT).map_err(serde::de::Error::custom)?;
        Ok(DateTime::<Utc>::from_naive_utc_and_offset(dt, Utc))
    }
}

pub(crate) mod json_string {
    use serde_json;
    use serde::de::{Deserialize, DeserializeOwned, Deserializer};

    pub fn deserialize<'de, T, D>(deserializer: D) -> Result<Option<T>, D::Error>
    where T: DeserializeOwned,
          D: Deserializer<'de>
    {
        let json = String::deserialize(deserializer)?;
        let result = serde_json::from_str(&json);

        match result {
            Ok(val) => Ok(val),
            // TODO: This will silently ignore any Podping deserialization errors
            // Theoretically that will exclude anything not compliant with the JSON schema
            _ => Ok(None)
        }
    }
}

#[derive(Deserialize, Debug)]
pub(crate) struct GetDynamicGlobalPropertiesResponse {
    // There are a lot more fields, but this is all we care about
    pub(crate) head_block_number: u64,
    #[serde(with = "hive_datetime_format")]
    pub(crate) time: DateTime<Utc>,
}

#[derive(Deserialize, Debug)]
pub(crate) struct GetBlockResponse {
    pub(crate) block: HiveBlock,
}

#[derive(Deserialize, Debug)]
pub(crate) struct HiveBlock {
    // There are a lot more fields, but this is all we care about
    pub(crate) block_id: String,
    #[serde(with = "hive_datetime_format")]
    pub(crate) timestamp: DateTime<Utc>,
    pub(crate) transaction_ids: Vec<String>,
    pub(crate) transactions: Vec<HiveTransaction>,
}

#[derive(Deserialize, Debug)]
pub(crate) struct HiveTransaction {
    // There are a lot more fields, but this is all we care about
    pub(crate) operations: Vec<HiveOperation>
}

#[serde_as]
#[derive(Deserialize, Debug)]
pub(crate) struct HiveOperation {
    #[serde(rename = "type")]
    pub(crate) type_: String,
    #[serde_as(deserialize_as = "DefaultOnError")]
    pub(crate) value: Option<PodpingOperation>
}

#[derive(Deserialize, Debug)]
pub(crate) struct PodpingOperation {
    pub(crate) id: Option<String>,
    #[serde(deserialize_with = "json_string::deserialize")]
    pub(crate) json: Option<Podping>
}