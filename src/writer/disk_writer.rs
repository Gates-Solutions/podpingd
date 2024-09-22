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
use std::fs::remove_dir_all;
use std::path::PathBuf;
use std::time::Duration;
use color_eyre::Report;
use tokio::sync::broadcast::Receiver;
use tracing::{debug, error, info, warn};
use tokio::task::JoinSet;
use podping_schemas::org::podcastindex::podping::podping_json::Podping;
use chrono::{DateTime, Datelike, NaiveDate, TimeZone, Timelike, Utc};
use regex::{Match, Regex};
use tokio::sync::broadcast::error::RecvError;
use tokio::time::Instant;
use walkdir::WalkDir;
use crate::config::Settings;
use crate::hive::scanner::HiveBlockWithNum;
use crate::writer::writer::LAST_UPDATED_BLOCK_FILENAME;
use crate::writer::writer::Writer;


async fn disk_write_block_transactions(data_dir_path: PathBuf, block: HiveBlockWithNum) -> color_eyre::Result<(), Report> {
    if block.transactions.is_empty() {
        info!("No Podpings for block {}", block.block_num);
    } else {
        let current_block_dir = data_dir_path
            .join(block.timestamp.year().to_string())
            .join(block.timestamp.month().to_string())
            .join(block.timestamp.day().to_string())
            .join(block.timestamp.hour().to_string())
            .join(block.timestamp.minute().to_string())
            .join(block.timestamp.second().to_string());

        let create_dir_future = tokio::fs::create_dir_all(&current_block_dir);

        let mut write_join_set = JoinSet::new();

        for tx in &block.transactions {
            for (i, podping) in tx.podpings.iter().enumerate() {
                let podping_file = match podping {
                    Podping::V0(_)
                    | Podping::V02(_)
                    | Podping::V03(_)
                    | Podping::V10(_) => current_block_dir
                        .join(format!("{}_{}_{}.json", block.block_num, tx.tx_id, i)),
                    Podping::V11(pp) => current_block_dir
                        .join(format!(
                            "{}_{}_{}_{}.json",
                            block.block_num,
                            tx.tx_id,
                            pp.session_id.to_string(),
                            pp.timestamp_ns.to_string())
                        ),
                };

                let json = serde_json::to_string(&podping);

                match json {
                    Ok(json) => {
                        info!("block: {}, tx: {}, podping: {}", block.block_num, tx.tx_id, json);

                        info!("Writing podping to file: {}", podping_file.to_string_lossy());
                        write_join_set.spawn(tokio::fs::write(podping_file, json));
                    }
                    Err(e) => {
                        error!("Error writing podping file {}: {}", podping_file.to_string_lossy(), e);
                    }
                }
            }
        }

        create_dir_future.await?;

        write_join_set.join_all().await;
    }
    Ok(())
}

pub enum TrimLevel {
    Year,
    Month,
    Day,
    Hour,
    Minute,
    Second
}

fn get_days_from_month(year: i32, month: u32) -> u32 {
    NaiveDate::from_ymd_opt(
        match month {
            12 => year + 1,
            _ => year,
        },
        match month {
            12 => 1,
            _ => month + 1,
        },
        1,
    )
        .unwrap()
        .signed_duration_since(NaiveDate::from_ymd_opt(year, month, 1).unwrap())
        .num_days() as u32
}

fn entry_path_to_date(entry_path: &PathBuf, date_regex: &Regex) -> Result<Option<DateTime<Utc>>, Report> {
    let entry_str = entry_path.to_string_lossy();
    let captures = date_regex.captures(&entry_str);

    let (year, month, day, hour, minute, second) = match captures {
        Some(caps) => {
            (
                match caps.name("year") {
                    Some(m) => m.as_str().parse::<u32>().unwrap_or(0),
                    None => 0
                },
                match caps.name("month") {
                    Some(m) => m.as_str().parse::<u32>().unwrap_or(12),
                    None => 12
                },
                match caps.name("day") {
                    Some(m) => m.as_str().parse::<u32>().unwrap_or(0),
                    None => 0
                },
                match caps.name("hour") {
                    Some(m) => m.as_str().parse::<u32>().unwrap_or(23),
                    None => 23
                },
                match caps.name("minute") {
                    Some(m) => m.as_str().parse::<u32>().unwrap_or(59),
                    None => 59
                },
                match caps.name("second") {
                    Some(m) => m.as_str().parse::<u32>().unwrap_or(59),
                    None => 59
                },
            )
        },
        None => return Ok(None)
    };

    let actual_day = match day {
        0 => get_days_from_month(year as i32, month),
        _ => day
    };

    Ok(Some(Utc.with_ymd_and_hms(year as i32, month, actual_day, hour, minute, second).unwrap()))
}

fn disk_trim_before(data_dir_path: &PathBuf, keep_after: DateTime<Utc>, trim_level: &Option<TrimLevel>) -> Result<(), Report> {
    let date_regex =  Regex::new(r"^.*?/(?<year>\d+)/?(?<month>\d+)?/?(?<day>\d+)?/?(?<hour>\d+)?/?(?<minute>\d+)?/?(?<second>\d+)?$")?;

    let level = match trim_level {
        Some(l) => l,
        None => return Ok(())
    };

    let next_level = match level {
        TrimLevel::Year => Some(TrimLevel::Month),
        TrimLevel::Month => Some(TrimLevel::Day),
        TrimLevel::Day => Some(TrimLevel::Hour),
        TrimLevel::Hour => Some(TrimLevel::Minute),
        TrimLevel::Minute => Some(TrimLevel::Second),
        TrimLevel::Second => None
    };

    for entry_result in WalkDir::new(data_dir_path).min_depth(1).max_depth(1) {
        let dir_entry = match entry_result {
            Ok(entry) => entry,
            Err(e) => {
                error!("Error trimming old disk files under {}: {}", data_dir_path.to_string_lossy(), e);
                break
            }
        };

        if !dir_entry.file_type().is_dir() {
            break
        }

        let entry_path = dir_entry.into_path();

        let pah_date = entry_path_to_date(&entry_path, &date_regex);

        match pah_date {
            Ok(entry_date) => {
                match entry_date {
                    Some(d) => {
                        if d < keep_after {
                            info!("disk trim: deleting directory {}", entry_path.to_string_lossy());
                            remove_dir_all(entry_path)?;
                        } else {
                            // Check if any children in the current entry will always be ahead
                            // This helps us avoid scanning everything
                            let skip_scan = match level {
                                TrimLevel::Year => d.year() > keep_after.year(),
                                TrimLevel::Month => d.month() > keep_after.month(),
                                TrimLevel::Day => d.day() > keep_after.day(),
                                TrimLevel::Hour => d.hour() > keep_after.hour(),
                                TrimLevel::Minute => d.minute() > keep_after.minute(),
                                TrimLevel::Second => d.second() > keep_after.second(),
                            };

                            if !skip_scan {
                                debug!("disk trim: scanning directory {}", entry_path.to_string_lossy());
                                disk_trim_before(&entry_path, keep_after, &next_level)?
                            } else {
                                debug!("disk trim: skipping directory {}", entry_path.to_string_lossy());
                            }
                        }
                    },
                    None => break
                }
            }
            _ => break
        };
    }

    Ok(())
}

pub fn disk_trim_old(data_dir_path: &PathBuf, keep_duration: Duration) -> Result<(), Report> {
    info!("disk trim: starting");
    let now = Utc::now();

    let keep_after = now - keep_duration;

    info!("disk trim: trimming podpings older than {}", keep_after);

    disk_trim_before(data_dir_path, keep_after, &Some(TrimLevel::Year))?;

    info!("disk trim: trimming complete");
    
    Ok(())
}


pub(crate) struct DiskWriter {
    directory: PathBuf,
    last_block_file: PathBuf,
    keep_duration: Option<Duration>
}

impl Writer for DiskWriter {
    fn new(settings: &Settings) -> Self
    where
        Self: Sized
    {
        let dir_path = match settings.writer.disk_directory.clone() {
            Some(disk_directory) => match disk_directory.is_empty() {
                true => panic!("Data directory is empty"),
                false => PathBuf::from(disk_directory),
            },
            None => panic!("Data directory is not set!")
        };

        if !dir_path.is_dir() {
            panic!("Data directory {} is not a directory.  Please ensure it exists", dir_path.display());
        }

        let last_block_file = dir_path.join(LAST_UPDATED_BLOCK_FILENAME);
        
        match settings.writer.disk_trim_old.unwrap_or(false) {
            true => DiskWriter {
                directory: dir_path,
                last_block_file,
                keep_duration: Some(
                    settings.writer.disk_trim_keep_duration.expect(
                        "disk_trim_old is enabled but disk_trim_keep_duration is not set!"
                    )
                )
            },
            false => DiskWriter {
                directory: dir_path,
                last_block_file,
                keep_duration: None
            }
        }
        
    }

    async fn get_last_block(&self) -> Option<u64> {
        match tokio::fs::read_to_string(&self.last_block_file).await {
            Ok(s) => match s.trim().parse::<u64>() {
                Ok(block) => Some(block),
                _ => None
            },
            _ => None
        }
    }

    async fn start(&self, mut rx: Receiver<HiveBlockWithNum>) -> color_eyre::Result<(), Report> {
        let mut start_time = Instant::now();
        
        loop {
            let result = rx.recv().await;

            let block = match result {
                Ok(block) => Some(block),
                Err(RecvError::Lagged(e)) => {
                    warn!("Disk writer is lagging: {}", e);

                    None
                }
                Err(RecvError::Closed) => {
                    panic!("Disk writer channel closed");
                }
            };

            // Trim old podpings every hour if the setting is enabled
            // TODO: Move this to a separate thread?
            // All this does is possibly cause extra buffering when trimming a large amount of files
            match self.keep_duration {
                Some(keep_duration) => {
                    let now = Instant::now();

                    if now - start_time >= Duration::from_secs(1*60*60) {
                        disk_trim_old(&self.directory.clone(), keep_duration)?;

                        start_time = Instant::now();
                    }
                }
                None => ()
            };
            
            match block {
                Some(block) => {
                    let block_num = block.block_num.to_owned();
                    disk_write_block_transactions(self.directory.clone(), block).await?;
                    tokio::fs::write(&self.last_block_file, block_num.to_string()).await?
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
                    warn!("Disk writer is lagging: {}", e);

                    None
                }
                Err(RecvError::Closed) => {
                    break
                }
            };

            match block {
                Some(blocks) => {
                    let last_block_num = &blocks.last().unwrap().block_num.to_string();
                    let mut write_join_set = JoinSet::new();

                    for block in blocks {
                        write_join_set.spawn(disk_write_block_transactions(self.directory.clone(), block));
                    }

                    write_join_set.join_all().await;

                    tokio::fs::write(&self.last_block_file, last_block_num.to_string()).await?;
                }
                None => {}
            }
        };

        Ok(())
    }
}