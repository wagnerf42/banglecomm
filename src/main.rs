use chrono::{NaiveDateTime, Utc};
use clap::Parser;
use directories_next::ProjectDirs;
use itertools::Itertools;
use std::fmt::Write;
use std::path::Path;
use std::sync::Arc;

use anyhow::Result;

use rustyline::error::ReadlineError;
use rustyline::DefaultEditor;

mod network;
use network::Communicator;
mod pairing;

mod cli;
use cli::Command;

pub mod utils;

#[tokio::main]
async fn main() -> Result<()> {
    let cli = cli::Cli::parse();

    let mut history_file = ProjectDirs::from("", "", "BangleComm")
        .map(|proj_dirs| proj_dirs.data_local_dir().to_path_buf())
        .unwrap_or_else(|| Path::new(".").to_path_buf());
    tokio::fs::create_dir_all(&history_file).await?;
    history_file.push("history.txt");

    let comms = Arc::new(Communicator::new().await?);

    // spawn the receiver
    let recv_comms = comms.clone();
    tokio::task::spawn(async {
        network::receive_messages(recv_comms)
            .await
            .expect("receiving messages failed")
    });

    if let Some(command) = cli.commands {
        let stay_alive = matches!(command, Command::App { .. });
        execute_cli_command(&comms, command).await?;
        if stay_alive {
            loop {
                tokio::time::sleep(std::time::Duration::new(5, 0)).await;
            }
        }
    } else {
        // sync the clock
        sync_clock(&comms).await?;

        // start the command line interface
        let mut rl = DefaultEditor::new()?;
        if rl.load_history(&history_file).is_err() {
            println!("No previous history.");
        }
        loop {
            let readline = rl.readline(">> ");
            match readline {
                Ok(line) => {
                    rl.add_history_entry(line.as_str())?;
                    if line == "exit" {
                        break;
                    }
                    match line.parse::<Command>() {
                        Err(_) => println!("we cannot parse command : {} ; available commands are 'get' 'put' 'ls' 'rm' 'run'", line),

                        Ok(command) => if let Err(e) = execute_cli_command(&comms, command).await {
                            eprintln!("failed: {}", e);
                        },
                    }
                }
                Err(ReadlineError::Interrupted) => {
                    println!("CTRL-C");
                    break;
                }
                Err(ReadlineError::Eof) => {
                    println!("CTRL-D");
                    break;
                }
                Err(err) => {
                    println!("Error: {:?}", err);
                    break;
                }
            }
        }
        rl.save_history(&history_file)?; // TODO: how to async ?
    }
    if !cli.keep_connected {
        comms.disconnect().await?;
    }

    Ok(())
}

async fn sync_clock(comms: &Communicator) -> Result<()> {
    // setTime((new Date("Tue, 19 Feb 2019 10:57")).getTime()/1000)
    let now = time::OffsetDateTime::now_utc();
    let now_in_secs = now.unix_timestamp();
    *comms.command.lock().await = Some(Command::SyncClock);
    let msg = format!("\x10setTime({});", now_in_secs);
    write(comms, &msg).await
}

async fn download(comms: &Communicator, filename: String) -> Result<()> {
    let msg = format!(
        "\x10let ab = require(\"Storage\").readArrayBuffer(\"{}\"); \
let buff = Uint8Array(ab, 0, ab.length) ;\
buff.forEach((c, i) => console.log('\\x10', c));",
        filename
    );
    *comms.command.lock().await = Some(Command::Get { filename });
    comms.send_message(&msg).await?;
    Ok(())
}

async fn upload(comms: &Communicator, filename: String) -> Result<()> {
    if filename.len() > 28 {
        eprintln!("this filename is too large (max 28 chars)");
        return Ok(());
    }
    let file_content = utils::read_file(&filename).await?;
    let mut chunks = file_content
        .chunks(1024)
        .enumerate()
        .map(|(i, chunk)| (i, chunk.iter().map(|d| d.to_string()).join(",")));
    let file_size = file_content.len();
    let first_chunk = chunks.next().ok_or_else(|| anyhow::anyhow!("empty file"))?;
    let mut msg = format!(
        "\x10require(\"Storage\").write(\"{}\", [{}], 0, {});",
        filename, first_chunk.1, file_size
    );
    msg.extend(chunks.map(|(index, chunk_msg)| {
        format!(
            "require(\"Storage\").write(\"{}\", [{}], {});",
            filename,
            chunk_msg,
            index * 1024
        )
    }));
    write(comms, &msg).await
}

async fn app(comms: &Communicator, filename: String) -> Result<()> {
    let uglified = tokio::process::Command::new("uglifyjs")
        .arg(&filename)
        .output()
        .await?;
    anyhow::ensure!(uglified.status.success(), "uglifyjs failed");
    let escaped_msg: String = String::from_utf8(uglified.stdout)
        .unwrap()
        .split_terminator('\n')
        .fold(String::new(), |mut s, line| {
            write!(&mut s, "\x10{}", line).ok();
            s
        });
    *comms.command.lock().await = Some(Command::App { filename });
    comms.send_message(&escaped_msg).await?;
    Ok(())
}

async fn run(comms: &Communicator, filename: String) -> Result<()> {
    let file_content = utils::read_file(&filename).await?;
    let msg = std::str::from_utf8(&file_content)?;
    let escaped_msg: String = msg
        .split_terminator('\n')
        .fold(String::new(), |mut s, line| {
            write!(&mut s, "\x10{}", line).ok();
            s
        });
    *comms.command.lock().await = Some(Command::Run { filename });
    comms.send_message(&escaped_msg).await?;
    Ok(())
}

// parse all events later than now and return summary, optional location and timestamp
async fn parse_ical_events(filename: &str) -> Result<Vec<(String, Option<String>, i64)>> {
    let file_content = utils::read_file(filename).await?;
    let ical = ical::parser::ical::IcalParser::new(file_content.as_slice())
        .next()
        .ok_or_else(|| anyhow::anyhow!("no calendar"))??;
    let now = chrono::Local::now();
    let mut events = Vec::new();
    for event in &ical.events {
        let mut dtstart = None;
        let mut summary = None;
        let mut location = None;
        for property in &event.properties {
            match property.name.as_str() {
                "DTSTART" => dtstart = property.value.as_ref(),
                "LOCATION" => location = property.value.as_ref(),
                "SUMMARY" => summary = property.value.as_ref(),
                _ => (),
            }
        }
        if let Some(start) = dtstart {
            let date = NaiveDateTime::parse_from_str(start, "%Y%m%dT%H%M%S%Z");
            if let Ok(date) = date {
                let date = date.and_local_timezone(Utc).unwrap();
                if date > now {
                    let summary = summary
                        .cloned()
                        .unwrap_or_else(|| "unknown event".to_string());
                    events.push((summary, location.cloned(), date.timestamp()));
                }
            }
        }
    }
    Ok(events)
}

// NOTE: this will erase all existing calendar events
async fn sync_calendar(comms: &Communicator, filename: String) -> Result<()> {
    let events = parse_ical_events(&filename).await?;
    *comms.command.lock().await = Some(Command::SyncCalendar {
        ical_filename: filename,
    });
    let mut msg = events.iter().fold(
        "\x10let e=[];".to_string(),
        |mut s, (summary, location, time)| {
            if let Some(location) = location {
                write!(
                    &mut s,
                    "e.push({{title:\"{}\", description: \"{}\", timestamp: {time}}});",
                    summary, location
                )
                .ok();
            } else {
                write!(
                    &mut s,
                    "e.push({{title:\"{}\", timestamp: {time}}});",
                    summary
                )
                .ok();
            };
            s
        },
    );
    msg.push_str("require(\"Storage\").writeJSON(\"android.calendar.json\", e);");
    comms.send_message(&msg).await?;
    Ok(())
}

async fn write(comms: &Communicator, code: &str) -> Result<()> {
    *comms.command.lock().await = None;
    comms.send_message(code).await?;
    Ok(())
}

async fn rm(comms: &Communicator, filename: String) -> Result<()> {
    let msg = format!("\x10require(\"Storage\").erase(\"{}\");", filename);
    write(comms, &msg).await
}

async fn ls(comms: &Communicator) -> Result<()> {
    write(
        comms,
        "\x10let l = require(\"Storage\").list(); l.forEach((f, i) => console.log(f));",
    )
    .await
}

async fn execute_cli_command(comms: &Communicator, command: Command) -> Result<()> {
    match command {
        Command::App { filename: f } => app(comms, f).await?,
        Command::Disconnect => (), // do nothing, we'll disconnect at the end
        Command::SyncClock => sync_clock(comms).await?,
        Command::Get { filename: f } => download(comms, f).await?,
        Command::Put { filename: f } => upload(comms, f).await?,
        Command::SyncCalendar { ical_filename: f } => sync_calendar(comms, f).await?,
        Command::Ls => ls(comms).await?,
        Command::Rm { filename: f } => rm(comms, f).await?,
        Command::Run { filename: f } => run(comms, f).await?,
        Command::Write { code: c } => write(comms, &c).await?,
    }
    Ok(())
}
