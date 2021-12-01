use clap::{App, Arg, SubCommand};
use colored::*;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::fs::File;
use std::io::{Error, Write};
use std::path::Path;
use std::process::{Command, Stdio};

#[derive(Debug, Serialize, Deserialize)]
struct Config {
    /// A map keyed by the location of each project, the value is another map with key/value pairs
    /// for the command name and the command + arguments to run.
    projects: HashMap<String, HashMap<String, String>>,

    /// This is not ideal, but currently the resolved HashMap is used to combine the current
    /// project's commands, merged with all the commands of parent projects.
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

    /// Get the current project's commands.
    /// Note: it will not merge the commands with any parent projects.
    fn get_project(&mut self, project: &str) -> Option<&mut HashMap<String, String>> {
        let path = fs::canonicalize(project).unwrap();
        let project = path.to_str().unwrap();

        if self.projects.contains_key(project) {
            self.projects.get_mut(project)
        } else {
            None
        }
    }

    /// Get the resolved commands, these are the commands of the current project, merged with all
    /// the parent projects.
    fn resolve_project_commands(&mut self, project: &str) -> Option<&mut HashMap<String, String>> {
        let path = fs::canonicalize(project).unwrap();
        let project = path.to_str().unwrap();

        let mut commands: HashMap<String, String> = HashMap::new();

        let mut parent: Vec<&str> = vec![];
        for part in path.iter() {
            parent.push(part.to_str().unwrap());
            let mut project_path = parent.join("/");

            // Drop double leading /
            if project_path.len() > 1 {
                project_path = (&project_path)[1..].to_owned();
            }

            // Merge commands with parent
            if self.projects.contains_key(&project_path) {
                for (key, value) in self.projects.get_mut(&project_path).unwrap() {
                    commands.insert(key.to_owned(), value.to_owned());
                }
            }
        }

        if !commands.is_empty() && !self.resolved.contains_key(project) {
            self.resolved.insert(project.to_owned(), HashMap::new());
        }

        if let Some(project) = self.resolved.get_mut(project) {
            for (key, value) in &commands {
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
        .arg(
            Arg::with_name("pwd")
                .long("pwd")
                .help("The current working directory")
                .default_value(current_dir.to_str().unwrap())
                .takes_value(true),
        )
        .arg(Arg::with_name("command").takes_value(false))
        .arg(
            Arg::with_name("arguments")
                .multiple(true)
                .takes_value(false)
                .help("Arguments to passthrough"),
        )
        .subcommand(
            SubCommand::with_name("add")
                .about("Add a new command")
                .arg(
                    Arg::with_name("pwd")
                        .long("pwd")
                        .help("The current working directory")
                        .default_value(current_dir.to_str().unwrap())
                        .takes_value(true),
                )
                .arg(
                    Arg::with_name("name")
                        .takes_value(true)
                        .help("Name of the command"),
                )
                .arg(
                    Arg::with_name("arguments")
                        .multiple(true)
                        .takes_value(false)
                        .help("Command to execute"),
                ),
        )
        .subcommand(
            SubCommand::with_name("rm")
                .about("Delete an existing command")
                .arg(
                    Arg::with_name("pwd")
                        .long("pwd")
                        .help("The current working directory")
                        .default_value(current_dir.to_str().unwrap())
                        .takes_value(true),
                )
                .arg(
                    Arg::with_name("name")
                        .takes_value(true)
                        .help("Name of the command"),
                ),
        )
        .subcommand(
            SubCommand::with_name("print")
                .about("Print the commands")
                .arg(
                    Arg::with_name("pwd")
                        .long("pwd")
                        .help("The current working directory")
                        .default_value(current_dir.to_str().unwrap())
                        .takes_value(true),
                )
                .arg(
                    Arg::with_name("json")
                        .help("Prints commands in JSON format")
                        .long("json")
                        .takes_value(false),
                ),
        );

    ensure_config_exists();

    let matches = cli.clone().get_matches();

    match matches.subcommand() {
        ("add", Some(add_matches)) => add(add_matches),
        ("rm", Some(del_matches)) => del(del_matches),
        ("print", Some(print_matches)) => print(print_matches),
        _ => {
            match matches.value_of("command") {
                Some(command) => {
                    let mut config = read_config();
                    let pwd = matches.value_of("pwd").unwrap();

                    if let Some(project) = config.resolve_project_commands(pwd) {
                        match project.get_mut(command) {
                            Some(args) => {
                                if matches.is_present("print") {
                                    // Actually print the command
                                    println!("{}", args);
                                } else {
                                    // Execute the command
                                    let mut cmd = Command::new("zsh");

                                    // Passthrough arguments
                                    if let Some(passthrough) = matches.values_of("arguments") {
                                        let command = passthrough.collect::<Vec<_>>().join(" ");

                                        // Attach arguments to existing command
                                        if !command.is_empty() {
                                            args.push(' ');
                                            args.push_str(&command);
                                        }
                                    }

                                    cmd.arg("-c").arg(args);

                                    cmd.stdin(Stdio::inherit())
                                        .stdout(Stdio::inherit())
                                        .stderr(Stdio::inherit())
                                        .output()
                                        .expect("failed to execute process");
                                }
                            }
                            None => {
                                // Project exists but command doesn't.
                                println!("Command `{}` does not exist.\n", command.blue());
                                print_project_commands(project);
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

fn print_project_commands(project: &HashMap<String, String>) {
    println!("Available commands:\n");
    let commands = project.len();

    // No commands
    if commands == 0 {
        println!("{}", " \u{2219} There are no commands available.\n".red());
    }

    // Commands
    for (key, value) in project {
        println!("  taco {}\n    {}\n", key.blue(), value.dimmed());
    }

    // Footer
    println!(
        "{}",
        format!(
            "{} command{}",
            commands,
            match commands {
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
                print_project_commands(project);
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
                    .resolve_project_commands(pwd)
                    .unwrap_or(&mut HashMap::new())
            )
            .unwrap()
        );
    } else {
        print_project_commands(
            config
                .resolve_project_commands(pwd)
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

// Currently using a library that automatically gives you the
// config dir, which does all the magic for you (including the $HOME, $XDG_CONFIG_HOME, ...).
// However, I'm on MacOS and I also want to use `~/.config`, but it results in
// `$HOME/Library/Application Support` instead, which sort of makes sense but I don't want that...
// Therefore doing this manually.
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

        // Write an empty config file
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
