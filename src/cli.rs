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

    /// Command to execute
    #[command(subcommand)]
    pub commands: Option<Command>,
}

#[derive(Subcommand, Clone)]
pub enum Command {
    Upload { filename: String },
    Download { filename: String },
    SyncClock,
    SyncCalendar { ical_filename: String },
    Ls,
    Disconnect,
    Rm { filename: String },
}

impl FromStr for Command {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let mut tokens = s.split_whitespace();
        let command_type = tokens.next().ok_or(())?;
        let arg = tokens.next();
        match command_type {
            "ls" => Ok(Command::Ls),
            "put" => Ok(Command::Upload {
                filename: arg.unwrap_or_default().to_string(),
            }),
            "get" => Ok(Command::Download {
                filename: arg.unwrap_or_default().to_string(),
            }),
            "rm" => Ok(Command::Rm {
                filename: arg.unwrap_or_default().to_string(),
            }),
            _ => Err(()),
        }
    }
}
