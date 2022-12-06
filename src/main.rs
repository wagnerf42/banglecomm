use std::collections::HashSet;
use std::sync::Arc;

use anyhow::Result;

use rustyline::error::ReadlineError;
use rustyline::Editor;

mod network;
use network::Communicator;
mod pairing;
use pairing::StdioPairingAgent;

// what type of commands
enum Command {
    Nothing,
    Ls,
    SyncClock,
    SyncCalendar,
    GetFile,
    PutFile,
    Run,
}

#[tokio::main]
async fn main() -> Result<()> {
    let comms = Arc::new(Communicator::new().await?);

    // spawn the receiver
    let recv_comms = comms.clone();
    tokio::task::spawn(network::receive_messages(recv_comms));

    // start the command line interface
    let mut rl = Editor::<()>::new()?;
    if rl.load_history("history.txt").is_err() {
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
                handle_command(&comms, line).await?;
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
    rl.save_history("history.txt")?;
    comms.disconnect().await?;

    Ok(())
}

async fn handle_command(comms: &Communicator, line: String) -> Result<()> {
    let mut tokens = line.split_whitespace();
    if let Some(command) = tokens.next() {
        let mut flags = HashSet::new();
        let mut args = Vec::new();
        for token in tokens {
            if token.starts_with('-') {
                flags.insert(token);
            } else {
                args.push(token);
            }
        }
        match command {
            "ls" => {
                let msg = if args.is_empty() {
                    "let files=require('Storage').list();for(let i=0;i<files.length;i++){console.log(files[i]);};".to_string()
                } else if args.len() == 1 {
                    format!("let files=require('Storage').list(/{}$/);for(let i=0;i<files.length;i++) {{console.log(files[i]);}};", escape(args[0]))
                } else {
                    todo!()
                };
                comms.send_message(&msg).await?;
            }
            "help" => {
                println!("commands are 'get' 'put' 'ls' 'run'");
            }
            _ => eprintln!("unknown command {}", command),
        }
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
