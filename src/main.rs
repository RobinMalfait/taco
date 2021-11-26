use clap::{App, Arg, SubCommand};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs::File;
use std::io::{Error, Write};
use std::path::Path;
use std::process::{Command, Stdio};

#[derive(Debug, Serialize, Deserialize)]
struct Config {
    aliases: HashMap<String, String>,
    projects: HashMap<String, HashMap<String, String>>,
}

impl Config {
    fn new() -> Self {
        Config {
            aliases: HashMap::new(),
            projects: HashMap::new(),
        }
    }

    fn resolve_project(&mut self, project: &str) -> Option<&mut HashMap<String, String>> {
        if let Some(project) = self.projects.get_mut(project) {
            Some(project)
        } else if let Some(project) = self.aliases.get(project) {
            println!("TODO: Resolve via alias... {}", project);
            None
        } else {
            None
        }
    }
}

fn main() -> Result<(), Error> {
    let current_dir = std::env::current_dir()?;

    let pwd_arg = Arg::with_name("pwd")
        .long("pwd")
        .help("The current working directory")
        .default_value(current_dir.to_str().unwrap())
        .takes_value(true);

    let mut cli = App::new("Taco")
        .version("1.0")
        .arg(
            Arg::with_name("config")
                .short("c")
                .long("config")
                .help("Sets a custom config file")
                .takes_value(true),
        )
        .arg(
            Arg::with_name("print")
                .short("p")
                .long("print")
                .takes_value(false),
        )
        .arg(&pwd_arg)
        .arg(Arg::with_name("command").takes_value(false))
        .subcommand(SubCommand::with_name("link").about("Make alias"))
        .subcommand(SubCommand::with_name("unlink").about("Unlink aliases"))
        .subcommand(
            SubCommand::with_name("add")
                .about("Add a new command")
                .arg(&pwd_arg)
                .arg(
                    Arg::with_name("name")
                        .takes_value(true)
                        .help("Name of the command"),
                )
                .arg(
                    Arg::with_name("arguments")
                        .multiple(true)
                        .takes_value(true)
                        .help("Command to execute"),
                ),
        )
        .subcommand(SubCommand::with_name("del").about("Delete an existing command"))
        .subcommand(
            SubCommand::with_name("print")
                .about("Print the commands")
                .arg(&pwd_arg),
        );

    ensure_config_exists();

    let matches = cli.clone().get_matches();

    match matches.subcommand() {
        ("link", Some(link_matches)) => link(link_matches),
        ("unlink", Some(unlink_matches)) => unlink(unlink_matches),
        ("add", Some(add_matches)) => add(add_matches),
        ("del", Some(del_matches)) => del(del_matches),
        ("print", Some(print_matches)) => print(print_matches),
        _ => {
            match matches.value_of("command") {
                Some(command) => {
                    let mut config = read_config();
                    let pwd = matches.value_of("pwd").unwrap();

                    if let Some(project) = config.resolve_project(pwd) {
                        let args = project.get(command).unwrap();

                        if matches.is_present("print") {
                            println!("{}", args);
                        } else {
                            Command::new("zsh")
                                .arg("-c")
                                .arg(args)
                                .stdin(Stdio::inherit())
                                .stdout(Stdio::inherit())
                                .stderr(Stdio::inherit())
                                .output()
                                .expect("failed to execute process");
                        }
                    } else {
                        cli.print_help().unwrap();
                        println!();
                    }
                }
                None => {
                    cli.print_help().unwrap();
                    println!();
                }
            }

            Ok(())
        }
    }
}

fn link(_matches: &clap::ArgMatches) -> Result<(), Error> {
    Ok(())
}

fn unlink(_matches: &clap::ArgMatches) -> Result<(), Error> {
    Ok(())
}

fn add(matches: &clap::ArgMatches) -> Result<(), Error> {
    let pwd = matches.value_of("pwd").unwrap();
    let mut config = read_config();

    let name = matches.value_of("name").unwrap();
    let command = matches
        .values_of("arguments")
        .unwrap()
        .collect::<Vec<_>>()
        .join(" ");

    match config.resolve_project(pwd) {
        Some(project) => {
            if let Some(existing) = project.get(name) {
                println!(
                    "Command \"{}\" already exists with value \"{}\"",
                    name, existing
                );

                match confirm(&format!("Do you want to override it with \"{}\"?", command)) {
                    true => {
                        // Passthrough
                    }
                    _ => {
                        println!("Aborted!");
                        return Ok(());
                    }
                }
            }

            project.insert(name.to_string(), command.clone());
            write_config(&config);
        }
        None => {
            let mut project = HashMap::new();
            project.insert(name.to_string(), command.clone());
            config.projects.insert(pwd.to_string(), project);
            write_config(&config);
        }
    }

    println!("Aliased \"{}\" to \"{}\" in {}", name, &command, pwd);

    Ok(())
}

fn del(_matches: &clap::ArgMatches) -> Result<(), Error> {
    Ok(())
}

fn print(matches: &clap::ArgMatches) -> Result<(), Error> {
    let pwd = matches.value_of("pwd").unwrap();
    let mut config = read_config();

    match config.resolve_project(pwd) {
        Some(project) => {
            println!("{:#?}", project);
        }
        None => {
            println!("Couldn't find project for \"{}\"", pwd);
        }
    }

    Ok(())
}

fn confirm(message: &str) -> bool {
    let mut s = String::new();
    print!("{} (y/N) ", message);
    let _ = std::io::stdout().flush();
    std::io::stdin()
        .read_line(&mut s)
        .expect("Did not enter a correct string");

    s.trim() == "y" || s.trim() == "Y" || s.trim() == ""
}

fn config_file_location() -> String {
    return Path::new(&dirs::home_dir().unwrap())
        .join(".config")
        .join("taco")
        .join("taco.json")
        .to_str()
        .unwrap()
        .to_owned();
}

fn ensure_config_exists() {
    let file_path = config_file_location();
    let location = Path::new(&file_path);

    if !location.exists() {
        // Ensure parent directories exist
        let prefix = location.parent().unwrap();
        std::fs::create_dir_all(prefix).unwrap();

        //
        write_config(&Config::new());
    }
}

fn read_config() -> Config {
    let file_path = config_file_location();
    let file = File::open(file_path).unwrap();
    let config: Config = serde_json::from_reader(file).expect("JSON was not well-formatted");

    config
}

fn write_config(config: &Config) {
    let file_path = config_file_location();
    std::fs::write(file_path, serde_json::to_string(&config).unwrap()).unwrap();
}
