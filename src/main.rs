use clap::Parser;
use directories_next::ProjectDirs;
use itertools::Itertools;
use std::path::Path;
use std::sync::Arc;

use anyhow::Result;

use rustyline::error::ReadlineError;
use rustyline::Editor;

mod network;
use network::Communicator;
mod pairing;
use pairing::StdioPairingAgent;

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
    tokio::task::spawn(network::receive_messages(recv_comms));

    if let Some(command) = cli.commands {
        execute_cli_command(&comms, command).await?;
    } else {
        // sync the clock
        sync_clock(&comms).await?;

        // start the command line interface
        let mut rl = Editor::<()>::new()?;
        if rl.load_history(&history_file).is_err() {
            println!("No previous history.");
        }
        loop {
            let readline = rl.readline(">> ");
            match readline {
                Ok(line) => {
                    rl.add_history_entry(line.as_str());
                    if line == "exit" {
                        break;
                    }
                    match line.parse::<Command>() {
                        Err(_) => println!("we cannot parse command : {} ; available commands are 'get' 'put' 'ls' 'rm' 'run'", line),

                        Ok(command) => execute_cli_command(&comms, command).await?,
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
    let msg = format!("setTime({})", now_in_secs);
    comms.send_message(&msg).await?;
    Ok(())
}

async fn download(comms: &Communicator, filename: String) -> Result<()> {
    let msg = format!(
        "let ab = require(\"Storage\").readArrayBuffer(\"{}\"); \
let buff = Uint8Array(ab, 0, ab.length) ;\
buff.forEach((c, i) => console.log(c));",
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
    let writes = file_content
        .chunks_exact(8)
        .enumerate()
        .map(|(i, chunk)| {
            let numbers = chunk.iter().map(|d| d.to_string()).join(",");
            format!("f.write(\"{}\", [{}], {});", filename, numbers, i * 8)
        })
        .collect::<String>();
    let msg = format!("let f = require(\"Storage\"); {}", writes);
    *comms.command.lock().await = Some(Command::Put { filename });
    comms.send_message(&msg).await?;
    Ok(())
}

async fn parse_ical_events(filename: &str) -> Result<Vec<(String, u32)>> {
    let file_content = utils::read_file(&filename).await?;
    todo!()
}

// require("sched").setAlarm("mydayalarm", { // as an alarm on a date
//   msg : "Wake up",
//   date : "2022-04-04",
//   t : 9 * 3600000 // 9 o'clock (in ms)
// });

// // Ensure the widget and alarm timer updates to schedule the new alarm properly
// require("sched").reload();
async fn sync_calendar(comms: &Communicator, filename: String) -> Result<()> {
    //TODO: should we mark events with a file id to remove if it gets cancelled ?
    let events = parse_ical_events(&filename).await?;
    *comms.command.lock().await = Some(Command::SyncCalendar {
        ical_filename: filename,
    });
    todo!();
    //comms.send_message(&msg).await?;
    Ok(())
}

async fn rm(comms: &Communicator, filename: String) -> Result<()> {
    let msg = format!("require(\"Storage\").erase(\"{}\");", filename);
    *comms.command.lock().await = Some(Command::Rm { filename });
    comms.send_message(&msg).await?;
    Ok(())
}

async fn ls(comms: &Communicator) -> Result<()> {
    let msg = "let l = require(\"Storage\").list(); l.forEach((f, i) => console.log(f));";
    *comms.command.lock().await = Some(Command::Ls);
    comms.send_message(msg).await?;
    Ok(())
}

async fn execute_cli_command(comms: &Communicator, command: Command) -> Result<()> {
    match command {
        Command::Disconnect => (), // do nothing, we'll disconnect at the end
        Command::SyncClock => sync_clock(comms).await?,
        Command::Get { filename: f } => download(comms, f).await?,
        Command::Put { filename: f } => upload(comms, f).await?,
        Command::SyncCalendar { ical_filename: f } => sync_calendar(comms, f).await?,
        Command::Ls => ls(comms).await?,
        Command::Rm { filename: f } => rm(comms, f).await?,
        _ => todo!(),
    }
    Ok(())
}

fn escape(string: &str) -> String {
    let mut escaped = String::new();
    for char in string.chars() {
        match char {
            '.' => {
                escaped += "\\.";
            }
            '*' => {
                escaped += "\\S+";
            }
            c => escaped.push(c),
        }
    }
    escaped
}
