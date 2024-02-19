use clap::{Parser, Subcommand};
use color_eyre::eyre::{eyre, Result};
use colored::*;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::fs::File;
use std::io::{Error, Write};
use std::path::Path;
use std::process::{Command, Stdio};

type Project = BTreeMap<String, String>;

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

    /// The subcommand to run
    #[clap(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Add a new command
    Add {
        /// The name of the alias for the command to run
        name: String,

        /// The actual command to run
        arguments: Vec<String>,
    },

    /// Alias the current project to a predefined project
    Alias {
        /// The name of the alias
        name: String,
    },

    /// Remove an existing command
    #[clap(name = "rm")]
    Remove {
        /// The name of the alias to remove
        name: String,
    },

    /// Print all the commands
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
    aliases: BTreeMap<String, Vec<String>>,

    /// A map keyed by the location of each project, the value is another map with key/value pairs
    /// for the command name and the command + arguments to run.
    #[serde(default)]
    projects: BTreeMap<String, Project>,
}

impl Config {
    fn new() -> Self {
        Config {
            aliases: BTreeMap::new(),
            projects: BTreeMap::new(),
        }
    }

    /// Get the list of aliases for a project
    fn add_alias(&mut self, project: &str, alias: &str) -> Result<()> {
        let path = fs::canonicalize(project)?;
        let key = path.to_str().unwrap();

        if !self.aliases.contains_key(key) {
            self.aliases.insert(key.to_string(), vec![]);
        }

        self.aliases.get_mut(key).unwrap().push(alias.to_string());

        Ok(())
    }

    /// Get the current project's commands.
    /// Note: it will not merge the commands with any parent projects.
    fn get_project_mut(&mut self, project: &str) -> Result<&mut Project> {
        let path = fs::canonicalize(project)?;

        match self.projects.get_mut(path.to_str().unwrap()) {
            Some(project) => Ok(project),
            None => Err(eyre!("Project not found: {}", project)),
        }
    }

    /// Get the resolved commands, these are the commands of the current project, merged with all
    /// the parent projects.
    fn resolve_project(&mut self, project: &str) -> Result<Project> {
        let path = fs::canonicalize(project)?;
        let mut commands: Project = BTreeMap::new();

        // Commands + aliases from parent directories
        let mut parent: Vec<&str> = vec![];
        for part in path.iter() {
            parent.push(part.to_str().unwrap());
            let mut project_path = parent.join("/");

            // Drop double leading /
            if project_path.len() > 1 {
                project_path = (&project_path)[1..].to_owned();
            }

            if let Some(other) = self.aliases.get(&project_path) {
                for alias in other {
                    if let Some(project) = self.projects.get(alias) {
                        for (key, value) in project {
                            commands.insert(key.to_owned(), value.to_owned());
                        }
                    }
                }
            }

            // Merge commands with parent
            if self.projects.contains_key(&project_path) {
                for (key, value) in self.projects.get_mut(&project_path).unwrap() {
                    commands.insert(key.to_owned(), value.to_owned());
                }
            }
        }

        Ok(commands)
    }
}

fn main() -> Result<()> {
    let args = Cli::parse();
    ensure_config_exists()?;

    let pwd = fs::canonicalize(&args.pwd)?.to_str().unwrap().to_string();

    match &args.command {
        Some(Commands::Add { name, arguments }) => {
            let mut config = read_config()?;
            let command = &arguments.join(" ");

            match config.get_project_mut(&pwd) {
                Ok(project) => {
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
                    write_config(&config)?;
                }
                Err(_) => {
                    let mut project = BTreeMap::new();
                    project.insert(name.to_string(), command.clone());
                    config.projects.insert(pwd.to_string(), project);
                    write_config(&config)?;
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
        Some(Commands::Alias { name }) => {
            let mut config = read_config()?;
            config.add_alias(&pwd, name)?;
            write_config(&config)?;
            println!("Added \"{}\" capabilities in {}", name.blue(), pwd.dimmed());
            Ok(())
        }
        Some(Commands::Remove { name }) => {
            let mut config = read_config()?;
            let project = config.get_project_mut(&pwd)?;
            match project.remove(name) {
                Some(_) => {
                    write_config(&config)?;
                    println!("Removed alias \"{}\"\n", name.blue());
                }
                None => {
                    println!("Alias \"{}\" does not exist.\n", name.blue());
                    print_project_commands(project);
                }
            }

            write_config(&config)?;

            Ok(())
        }
        Some(Commands::Print { json }) => {
            let mut config = read_config()?;

            if *json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&config.resolve_project(&pwd)?)?
                );
            } else {
                print_project_commands(&config.resolve_project(&pwd)?)
            }

            Ok(())
        }
        None => {
            if args.alias.is_none() {
                print_help()?;
            }

            let mut config = read_config()?;
            let alias = &args.alias.unwrap();
            let pwd = &args.pwd;
            let print = args.print;
            let arguments = args.arguments;
            let mut project = config.resolve_project(pwd)?;

            match project.get_mut(alias) {
                Some(args) if print => {
                    // Actually print the command
                    println!("{}", args);
                }
                Some(args) => {
                    let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_string());

                    // Execute the command
                    let mut cmd = Command::new(&shell);
                    cmd.current_dir(pwd);

                    // Passthrough arguments
                    let command = arguments.join(" ");

                    // Attach arguments to existing command
                    if !command.is_empty() {
                        args.push(' ');
                        args.push_str(&command);
                    }

                    // Add common flags for different shells
                    let cmd = match shell.as_str() {
                        "/bin/zsh" => cmd.arg("-i").arg("-c"),
                        "/bin/sh" => cmd.arg("-c"),
                        _ => &mut cmd,
                    };

                    cmd.arg(args);

                    if let Some(code) = cmd
                        .stdin(Stdio::inherit())
                        .stdout(Stdio::inherit())
                        .stderr(Stdio::inherit())
                        .output()
                        .expect("failed to execute process")
                        .status
                        .code()
                    {
                        std::process::exit(code);
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
    Path::new(&dirs::home_dir().unwrap())
        .join(".config")
        .join("taco")
        .join("taco.json")
        .to_str()
        .unwrap()
        .to_owned()
}

fn ensure_config_exists() -> Result<()> {
    let file_path = config_file_location();
    let location = Path::new(&file_path);

    if !location.exists() {
        // Ensure parent directories exist
        let prefix = location.parent().unwrap();
        std::fs::create_dir_all(prefix)?;

        // Write an empty config file
        write_config(&Config::new())?;
    }

    Ok(())
}

fn read_config() -> Result<Config> {
    let file_path = config_file_location();
    let file = File::open(file_path)?;
    let config: Config = serde_json::from_reader(file).expect("JSON was not well-formatted");

    Ok(config)
}

fn write_config(config: &Config) -> Result<()> {
    let file_path = config_file_location();
    std::fs::write(file_path, serde_json::to_string_pretty(&config)?)?;
    Ok(())
}
