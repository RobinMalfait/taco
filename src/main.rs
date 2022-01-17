use clap::{AppSettings, Parser, Subcommand};
use colored::*;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::fs::File;
use std::io::{Error, Write};
use std::path::Path;
use std::process::{Command, Stdio};

type Project = HashMap<String, String>;

/// Normalize all your commands by wrapping them in a taco
#[derive(Parser, Debug)]
#[clap(about, version, author)]
struct Cli {
    /// The current working directory
    #[clap(long, default_value = ".", global = true)]
    pwd: String,

    /// Print the current command instead of executing it
    #[clap(short, long)]
    print: bool,

    /// The alias to execute
    alias: Option<String>,

    /// The arguments to pass to the command
    arguments: Vec<String>,

    /// The changelog filename
    #[clap(short, long, default_value = "CHANGELOG.md", global = true)]
    filename: String,

    /// The subcommand to run
    #[clap(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Add a new command
    #[clap(setting(AppSettings::ArgRequiredElseHelp))]
    Add {
        /// The name of the alias for the command to run
        name: String,

        /// The actual command to run
        arguments: Vec<String>,
    },

    /// Remove an existing command
    #[clap(name = "rm", setting(AppSettings::ArgRequiredElseHelp))]
    Remove {
        /// The name of the alias to remove
        name: String,
    },

    /// Print all the commands
    #[clap(setting(AppSettings::ArgRequiredElseHelp))]
    Print {
        /// Print commands in JSON format
        #[clap(short, long)]
        json: bool,
    },
}

#[derive(Debug, Serialize, Deserialize)]
struct Config {
    /// A project can map to other projects so that it can inherit values from that other project.
    /// This allows you to define some common projects like "webdev" or "rust" or anything you
    /// want.
    #[serde(default)]
    aliases: HashMap<String, Vec<String>>,

    /// A map keyed by the location of each project, the value is another map with key/value pairs
    /// for the command name and the command + arguments to run.
    #[serde(default)]
    projects: HashMap<String, Project>,
}

impl Config {
    fn new() -> Self {
        Config {
            aliases: HashMap::new(),
            projects: HashMap::new(),
        }
    }

    /// Get the current project's commands.
    /// Note: it will not merge the commands with any parent projects.
    fn get_project_mut(&mut self, project: &str) -> Option<&mut Project> {
        let path = fs::canonicalize(project).unwrap();
        self.projects.get_mut(path.to_str().unwrap())
    }

    /// Get the resolved commands, these are the commands of the current project, merged with all
    /// the parent projects.
    fn resolve_project(&mut self, project: &str) -> Project {
        let path = fs::canonicalize(project).unwrap();
        let project = path.to_str().unwrap();
        let mut resolved: HashMap<String, Project> = HashMap::new();

        let mut commands: Project = HashMap::new();

        // Commands + aliases from parent directories
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

        if !commands.is_empty() && !resolved.contains_key(project) {
            resolved.insert(project.to_owned(), HashMap::new());
        }

        if let Some(project) = resolved.get_mut(project) {
            for (key, value) in &commands {
                project.insert(key.to_owned(), value.to_owned());
            }
        }

        // aliases from other directories
        // let main = resolved.get_mut(project);
        if let Some(other) = self.aliases.get(project) {
            for alias in other {
                if let Some(project) = self.projects.get(alias) {
                    for (key, value) in project {
                        if !commands.contains_key(key) {
                            commands.insert(key.to_owned(), value.to_owned());
                        }
                    }
                }
            }
        }

        commands
    }
}

fn main() -> Result<(), Error> {
    let args = Cli::parse();
    ensure_config_exists();

    let pwd = fs::canonicalize(&args.pwd)?.to_str().unwrap().to_string();

    match &args.command {
        Some(Commands::Add { name, arguments }) => {
            let mut config = read_config();
            let command = &arguments.join(" ");

            match config.get_project_mut(&pwd) {
                Some(project) => {
                    if let Some(existing) = project.get(name) {
                        println!(
                            "Command \"{}\" already exists with value \"{}\"",
                            name.blue(),
                            existing.blue()
                        );

                        if !confirm(&format!(
                            "Do you want to override it with \"{}\"?",
                            command.blue()
                        )) {
                            println!("{}", "Aborted!".red());
                            return Ok(());
                        }
                    }

                    // Akshually insert the new command.
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

            println!(
                "Aliased \"{}\" to \"{}\" in {}",
                name.blue(),
                &command.blue(),
                pwd.dimmed()
            );
            Ok(())
        }
        Some(Commands::Remove { name }) => {
            let mut config = read_config();

            match config.get_project_mut(&pwd) {
                Some(project) => {
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
                None => {
                    println!(
                        "Project \"{}\" does not exist, therefore \"{}\" doesn't exist either.",
                        pwd.dimmed(),
                        name.blue()
                    );
                }
            }

            Ok(())
        }
        Some(Commands::Print { json }) => {
            let mut config = read_config();

            if *json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&config.resolve_project(&pwd)).unwrap()
                );
            } else {
                print_project_commands(&config.resolve_project(&pwd))
            }

            Ok(())
        }
        None => {
            if args.alias.is_none() {
                print_help()?;
            }

            let mut config = read_config();
            let alias = &args.alias.unwrap();
            let pwd = &args.pwd;
            let print = args.print;
            let arguments = args.arguments;
            let mut project = config.resolve_project(pwd);

            match project.get_mut(alias) {
                Some(args) => {
                    if print {
                        // Actually print the command
                        println!("{}", args);
                    } else {
                        // Execute the command
                        let mut cmd = Command::new(
                            std::env::var("SHELL").unwrap_or_else(|_| "sh".to_string()),
                        );
                        cmd.current_dir(pwd);

                        // Passthrough arguments
                        let command = arguments.join(" ");

                        // Attach arguments to existing command
                        if !command.is_empty() {
                            args.push(' ');
                            args.push_str(&command);
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
                    println!("Command `{}` does not exist.\n", alias.blue());
                    print_project_commands(&project);
                }
            }

            Ok(())
        }
    }
}

fn print_project_commands(project: &Project) {
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

fn confirm(message: &str) -> bool {
    let mut s = String::new();
    print!("{} {} ", message, "(y/N)".dimmed());
    let _ = std::io::stdout().flush();
    std::io::stdin()
        .read_line(&mut s)
        .expect("Did not enter a correct string");

    println!();

    s.trim() == "y" || s.trim() == "Y"
}

fn print_help() -> Result<(), Error> {
    let mut cmd = Command::new(std::env::current_exe()?);

    cmd.arg("--help");

    cmd.stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .output()
        .expect("failed to execute process");

    std::process::exit(0);
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
