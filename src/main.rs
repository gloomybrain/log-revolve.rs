use async_std::fs::File;
use async_std::fs::OpenOptions;
use async_std::io;
use async_std::path::{Path, PathBuf};
use async_std::prelude::*;
use async_std::task::block_on;

use chrono::{prelude::*, DateTime, Duration, Local};

use std::collections::BTreeMap;

use structopt::StructOpt;

#[derive(StructOpt)]
#[structopt(rename_all = "kebab_case")]
struct CliOptions {
    #[structopt(long)]
    log_dir: String,

    #[structopt(long)]
    accepted_log_channels: String,

    #[structopt(long, default_value = "inapt")]
    inapt_file_name: String,
}

fn main() {
    println!("log-revolve-rs started");

    block_on(start()).expect("Something went terribly wrong");

    println!("log-revolve-rs finished");
}

async fn start() -> Result<(), io::Error> {
    let cli_options = CliOptions::from_args();

    let stdin = io::stdin();
    let mut line = String::new();
    let mut writer = FileWriter::with_options(&cli_options).await?;

    loop {
        stdin.read_line(&mut line).await?;

        writer.write(&line).await?;

        line.clear();
    }
}

struct FileHandle {
    file_name: String,
    log_dir: String,
    last_reopened: DateTime<Local>,
    current_file: File,
}

impl FileHandle {
    async fn open_file(path_str: &str) -> Result<File, io::Error> {
        let path_string = String::from(path_str);
        let path = Path::new(&path_string);

        OpenOptions::new()
            .create(true)
            .truncate(false)
            .append(true)
            .read(false)
            .open(path)
            .await
    }

    async fn create(log_dir: &str, channel_name: &str) -> Result<Self, io::Error> {
        let now = FileHandle::get_hourly_aligned_date();
        let path = FileHandle::generate_file_path(log_dir, channel_name, now)?;
        let file = FileHandle::open_file(path.as_str()).await?;

        Ok(FileHandle {
            file_name: channel_name.to_string(),
            last_reopened: Local::now(),
            log_dir: path.to_string(),
            current_file: file,
        })
    }

    fn get_hourly_aligned_date() -> DateTime<Local> {
        let now = Local::now();
        Local
            .ymd(now.year(), now.month(), now.day())
            .and_hms(now.hour(), 0u32, 0u32)
    }

    fn generate_file_path(
        log_dir: &str,
        channel_name: &str,
        now: DateTime<Local>,
    ) -> Result<String, io::Error> {
        let mut file_name = String::new();
        file_name.push_str(channel_name);
        file_name.push_str("_");
        file_name.push_str(&now.format("%Y-%m-%d-%H-%M-%S").to_string());
        file_name.push_str(".log");

        let mut path_buf = PathBuf::new();
        path_buf.push(log_dir);
        path_buf.push(file_name);

        let path_str_opt = path_buf.to_str();
        match path_str_opt {
            Some(path_str) => Ok(path_str.to_string()),
            None => Err(io::Error::new(
                io::ErrorKind::Other,
                "unable to build file path",
            )),
        }
    }

    async fn write_line(&mut self, line: &str) -> Result<(), io::Error> {
        self.update_current_file().await?;
        self.current_file.write_all(line.as_bytes()).await
    }

    async fn update_current_file(&mut self) -> Result<(), io::Error> {
        if self.new_file_needed() {
            self.last_reopened = FileHandle::get_hourly_aligned_date();
            let path_str =
                FileHandle::generate_file_path(&self.log_dir, &self.file_name, self.last_reopened)?;
            self.current_file = FileHandle::open_file(path_str.as_str()).await?
        }

        Ok(())
    }

    fn new_file_needed(&self) -> bool {
        let now = Local::now();

        let date_is_after = now.date() > self.last_reopened.date();
        let hour_is_after = now.hour() > self.last_reopened.hour();
        if date_is_after && hour_is_after {
            return true;
        }

        let more_than_hour_passed = now - self.last_reopened > Duration::hours(1);
        if more_than_hour_passed {
            return true;
        }

        false
    }
}

struct FileWriter {
    current_channel_name: Option<String>,
    inapt_file_handle: FileHandle,
    file_handles: BTreeMap<String, FileHandle>,
}

impl FileWriter {
    async fn with_options(options: &CliOptions) -> Result<Self, io::Error> {
        let mut file_handles = BTreeMap::new();

        let accepted_channels: Vec<String> = options
            .accepted_log_channels
            .split(",")
            .map(|s| s.to_string())
            .collect();

        let mut iterator = accepted_channels.iter();
        while let Some(channel_name) = iterator.next() {
            let handle = FileHandle::create(&options.log_dir, channel_name).await?;
            file_handles.insert(channel_name.clone(), handle);
        }

        let inapt_file_handle =
            FileHandle::create(&options.log_dir, &options.inapt_file_name).await?;

        Ok(FileWriter {
            current_channel_name: Option::None,
            inapt_file_handle,
            file_handles,
        })
    }

    async fn write(&mut self, message: &str) -> Result<(), io::Error> {
        match self.current_channel_name {
            None => {
                let channel = message.trim_end();
                if self.file_handles.contains_key(channel) {
                    self.current_channel_name = Some(channel.clone().to_string());

                    Ok(())
                } else {
                    self.inapt_file_handle.write_line(message).await
                }
            }
            Some(ref channel) => {
                let handle = self
                    .file_handles
                    .get_mut(channel)
                    .unwrap_or(&mut self.inapt_file_handle);

                handle.write_line(message).await?;

                self.current_channel_name = None;
                Ok(())
            }
        }
    }
}
