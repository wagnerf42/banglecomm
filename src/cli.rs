use std::str::FromStr;

use clap::{Parser, Subcommand};
#[derive(Parser)]
#[command(name = "BangleComm")]
#[command(author = "frederic wagner <frederic.wagner@imag.fr>")]
#[command(version = "0.1")]
#[command(about = "Command line interface to the banglejs watch")]
#[command(long_about = None)]
pub struct Cli {
    /// Display more information on what's happening for debug purposes.
    #[arg(short, long)]
    pub verbose: bool,
    /// Don't close connection when exiting.
    #[arg(short, long)]
    pub keep_connected: bool,

    /// Command to execute.
    #[command(subcommand)]
    pub commands: Option<Command>,
}

#[derive(Subcommand, Clone)]
pub enum Command {
    /// Upload given file to the watch.
    Put { filename: String },
    /// Download given file from the watch.
    Get { filename: String },
    /// Synchronize the watch with the local time.
    SyncClock,
    /// Add ical file's events as alarms.
    SyncCalendar { ical_filename: String },
    /// List files.
    Ls,
    /// Close connection.
    Disconnect,
    /// Erase given file.
    Rm { filename: String },
    /// Run given js file on the watch.
    Run { filename: String },
    /// Run given code line on the watch.
    Write { code: String },
}

impl FromStr for Command {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let mut tokens = s.split_whitespace();
        let command_type = tokens.next().ok_or(())?;
        let arg = tokens.next().unwrap_or_default().to_string();
        match command_type {
            "ls" => Ok(Command::Ls),
            "put" => Ok(Command::Put { filename: arg }),
            "get" => Ok(Command::Get { filename: arg }),
            "rm" => Ok(Command::Rm { filename: arg }),
            "run" => Ok(Command::Run { filename: arg }),
            _ => Err(()),
        }
    }
}
