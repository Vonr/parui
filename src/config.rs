use std::{env::Args, process::exit};

use self::help::print_help;

mod help;

pub struct Config {
    pub query: Option<String>,
    pub command: String,
}

impl Config {
    pub fn new(args: Args) -> Self {
        let mut query: Option<String> = None;
        let mut command = String::from("paru");

        for arg in args.skip(1) {
            match arg.as_str() {
                "-h" | "--help" => print_help(),
                _ => {
                    if arg.starts_with("-p=") {
                        command = arg.clone().chars().skip(3).collect::<String>();
                    } else if let Some(q) = query {
                        query = Some(q + " " + &arg);
                    } else {
                        query = Some(arg.to_owned());
                    }
                }
            }
        }

        if let Err(err) = std::process::Command::new(command.as_str())
            .arg("-h")
            .output()
        {
            match err.kind() {
                std::io::ErrorKind::NotFound => {
                    eprintln!("parui: {command}: command not found");
                }
                _ => {
                    eprintln!("parui: {command}: {err}");
                }
            }
            exit(1);
        }

        Self { query, command }
    }
}
