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
        let mut command = None;

        for arg in args.skip(1) {
            match arg.as_str() {
                "-h" | "--help" => print_help(),
                #[allow(clippy::option_if_let_else)]
                _ => {
                    if let Some(stripped) = arg.strip_prefix("-p=") {
                        command = Some(stripped.to_string());
                    } else if let Some(q) = query {
                        query = Some(q + " " + &arg);
                    } else {
                        query = Some(arg.to_owned());
                    }
                }
            }
        }

        let command = command.unwrap_or_else(|| String::from("paru"));

        if let Err(err) = std::process::Command::new(&command).arg("-h").output() {
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
