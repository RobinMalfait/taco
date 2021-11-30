use clap::{App, Arg, SubCommand};
use colored::*;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::fs::File;
use std::io::{Error, Write};
use std::path::Path;
use std::process::{Command, Stdio};

#[derive(Debug, Serialize, Deserialize, Clone)]
struct Config {
    projects: HashMap<String, HashMap<String, String>>,

    #[serde(skip)]
    resolved: HashMap<String, HashMap<String, String>>,
}

impl Config {
    fn new() -> Self {
        Config {
            projects: HashMap::new(),
            resolved: HashMap::new(),
        }
    }

    fn get_project(&mut self, project: &str) -> Option<&mut HashMap<String, String>> {
        let path = fs::canonicalize(project).unwrap();
        let project = path.to_str().unwrap();

        if self.projects.contains_key(project) {
            self.projects.get_mut(project)
        } else {
            None
        }
    }

    fn resolve_project_scripts(&mut self, project: &str) -> Option<&mut HashMap<String, String>> {
        let path = fs::canonicalize(project).unwrap();
        let project = path.to_str().unwrap();

        let mut scripts: HashMap<String, String> = HashMap::new();

        let mut parent: Vec<&str> = vec![];
        for part in path.iter() {
            parent.push(part.to_str().unwrap());
            let mut project_path = parent.join("/");

            // Drop double leading /
            if project_path.len() > 1 {
                project_path = (&project_path)[1..].to_owned();
            }

            // Merge scripts with parent
            if self.projects.contains_key(&project_path) {
                for (key, value) in self.projects.get_mut(&project_path).unwrap() {
                    scripts.insert(key.to_owned(), value.to_owned());
                }
            }
        }

        if !scripts.is_empty() && !self.resolved.contains_key(project) {
            self.resolved.insert(project.to_owned(), HashMap::new());
        }

        if let Some(project) = self.resolved.get_mut(project) {
            for (key, value) in &scripts {
                project.insert(key.to_owned(), value.to_owned());
            }

            Some(project)
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
        .subcommand(
            SubCommand::with_name("del")
                .about("Delete an existing command")
                .arg(&pwd_arg)
                .arg(
                    Arg::with_name("name")
                        .takes_value(true)
                        .help("Name of the command"),
                ),
        )
        .subcommand(
            SubCommand::with_name("print")
                .about("Print the commands")
                .arg(&pwd_arg)
                .arg(
                    Arg::with_name("json")
                        .help("Prints scripts in JSON format")
                        .long("json")
                        .takes_value(false),
                ),
        );

    ensure_config_exists();

    let matches = cli.clone().get_matches();

    match matches.subcommand() {
        ("add", Some(add_matches)) => add(add_matches),
        ("del", Some(del_matches)) => del(del_matches),
        ("print", Some(print_matches)) => print(print_matches),
        _ => {
            match matches.value_of("command") {
                Some(command) => {
                    let mut config = read_config();
                    let pwd = matches.value_of("pwd").unwrap();

                    if let Some(project) = config.resolve_project_scripts(pwd) {
                        match project.get(command) {
                            Some(args) => {
                                if matches.is_present("print") {
                                    // Actually print the command
                                    println!("{}", args);
                                } else {
                                    // Execute the command
                                    Command::new("zsh")
                                        .arg("-c")
                                        .arg(args)
                                        .stdin(Stdio::inherit())
                                        .stdout(Stdio::inherit())
                                        .stderr(Stdio::inherit())
                                        .output()
                                        .expect("failed to execute process");
                                }
                            }
                            None => {
                                // Project exists but command doesn't.
                                println!("Command `{}` does not exist.\n", command.blue());
                                print_project_scripts(project);
                            }
                        }
                    } else {
                        // Command provided, but project doesn't exist, just print the help
                        cli.print_help().unwrap();
                        println!();
                    }
                }
                None => {
                    // No command has been specified, just print the help
                    cli.print_help().unwrap();
                    println!();
                }
            }

            Ok(())
        }
    }
}

fn print_project_scripts(project: &HashMap<String, String>) {
    println!("Available scripts:\n");
    let scripts = project.len();

    // No scripts
    if scripts == 0 {
        println!("{}", " \u{2219} There are no scripts available.\n".red());
    }

    // Scripts
    for (key, value) in project {
        println!("  taco {}\n    {}\n", key.blue(), value.dimmed());
    }

    // Footer
    println!(
        "{}",
        format!(
            "{} script{}",
            scripts,
            match scripts {
                1 => "",
                _ => "s",
            }
        )
        .dimmed()
    );
}

fn add(matches: &clap::ArgMatches) -> Result<(), Error> {
    let pwd = matches.value_of("pwd").unwrap();
    let mut config = read_config();

    match matches.value_of("name") {
        Some(name) => {
            let command = matches
                .values_of("arguments")
                .unwrap()
                .collect::<Vec<_>>()
                .join(" ");

            match config.get_project(pwd) {
                Some(project) => {
                    if let Some(existing) = project.get(name) {
                        println!(
                            "Command \"{}\" already exists with value \"{}\"",
                            name, existing
                        );

                        match confirm(&format!("Do you want to override it with \"{}\"?", command))
                        {
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
        }
        None => {
            println!("No command provided.\nUsage:\n");
            println!("  taco add {} -- {}", "name".blue(), "commands".blue());

            println!("\nExample:");
            println!(
                "  taco add {} -- {}",
                "publish".blue(),
                "npm publish".blue()
            );
        }
    }

    Ok(())
}

fn del(matches: &clap::ArgMatches) -> Result<(), Error> {
    let pwd = matches.value_of("pwd").unwrap();
    let mut config = read_config();

    let name = matches.value_of("name").unwrap();

    if let Some(project) = config.get_project(pwd) {
        match project.remove(name) {
            Some(_) => {
                write_config(&config);
                println!("Removed alias \"{}\"\n", name.blue());
            }
            None => {
                println!("Alias \"{}\" does not exist.\n", name.blue());
                print_project_scripts(project);
            }
        }

        write_config(&config);
    }

    Ok(())
}

fn print(matches: &clap::ArgMatches) -> Result<(), Error> {
    let pwd = matches.value_of("pwd").unwrap();
    let mut config = read_config();

    if matches.is_present("json") {
        println!(
            "{}",
            serde_json::to_string_pretty(
                config
                    .resolve_project_scripts(pwd)
                    .unwrap_or(&mut HashMap::new())
            )
            .unwrap()
        );
    } else {
        print_project_scripts(
            config
                .resolve_project_scripts(pwd)
                .unwrap_or(&mut HashMap::new()),
        );
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
    std::fs::write(file_path, serde_json::to_string_pretty(&config).unwrap()).unwrap();
}
