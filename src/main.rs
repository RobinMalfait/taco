use clap::{Parser, Subcommand};
use color_eyre::eyre::{Result, eyre};
use colored::*;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::fs::File;
use std::io::{Error, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
mod rich_edit;
use crate::rich_edit::rich_edit;

type Project = BTreeMap<String, String>;

/// Normalize all your commands by wrapping them in a taco
#[derive(Parser, Debug)]
#[clap(about, version, author)]
struct Cli {
    /// The current working directory
    #[clap(long, default_value = ".", global = true)]
    pwd: PathBuf,

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
        arguments: Option<Vec<String>>,
    },

    /// Edit a command
    Edit {
        /// The name of the alias to edit
        name: String,
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

#[derive(Debug, Default, Serialize, Deserialize)]
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
    /// Get the list of aliases for a project
    fn add_alias(&mut self, project: &str, alias: &str) -> Result<()> {
        let path = fs::canonicalize(project)?;
        let key = path.to_str().unwrap();

        self.aliases
            .entry(key.into())
            .or_insert(vec![])
            .push(alias.into());

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
                project_path = (&project_path)[1..].into();
            }

            if let Some(other) = self.aliases.get(&project_path) {
                for alias in other {
                    if let Some(project) = self.projects.get(alias) {
                        commands.extend(project.clone());
                    }
                }
            }

            // Merge commands with parent
            if let Some(project) = self.projects.get(&project_path) {
                commands.extend(project.clone());
            }
        }

        Ok(commands)
    }
}

fn main() -> Result<()> {
    let args = Cli::parse();
    ensure_config_exists()?;

    let pwd = fs::canonicalize(&args.pwd)?.to_str().unwrap().to_string();

    let Some(command) = args.command else {
        if args.alias.is_none() {
            print_help()?;
        }

        let mut config = read_config()?;
        let alias = &args.alias.unwrap();
        let pwd = &args.pwd;
        let print = args.print;
        let arguments = args.arguments;
        let project = config.resolve_project(pwd.into())?;

        match project.get(alias) {
            Some(args) if print => {
                // Actually print the command
                println!("{}", args);
            }
            Some(args) => {
                let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_string());

                // Execute the command
                let mut cmd = Command::new(&shell);
                cmd.current_dir(pwd);
                let command = build_shell_command(args);

                // Add common flags for different shells
                let cmd = match shell.as_str() {
                    "/bin/zsh" => cmd.arg("-i").arg("-c"),
                    "/bin/sh" => cmd.arg("-c"),
                    _ => &mut cmd,
                };

                cmd.arg(command).arg("taco").args(arguments);

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

        return Ok(());
    };

    match command {
        Commands::Add { name, arguments } => {
            let mut config = read_config()?;
            let command = match arguments {
                Some(args) => args.join(" "),
                None => {
                    let Some(data) = rich_edit(
                        "\n# Enter the command you want to alias here.\n# Lines starting with '#' are ignored.\n",
                    ) else {
                        println!("{}", "Aborted!".red());
                        return Ok(());
                    };

                    let data: Vec<_> = data
                        .trim()
                        .lines()
                        .map(|line| line.trim())
                        .filter(|line| !line.starts_with('#'))
                        .collect();

                    if data.is_empty() {
                        println!("{}", "Aborted!".red());
                        return Ok(());
                    }

                    data.join("\n")
                }
            };

            match config.get_project_mut(&pwd) {
                Ok(project) => {
                    if let Some(existing) = project.get(&name) {
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
        }
        Commands::Edit { name } => {
            let mut config = read_config()?;

            let combined_project = config.resolve_project(&pwd)?;
            let Some(current_command) = combined_project.get(&name) else {
                println!(
                    "{}",
                    format!("Command \"{}\" does not exist, cannot edit it.", name).red()
                );
                return Ok(());
            };

            let Some(data) = rich_edit(&format!(
                "{}\n# Enter the command you want to alias here.\n# Lines starting with '#' are ignored.\n",
                current_command
            )) else {
                println!("{}", "Aborted!".red());
                return Ok(());
            };

            let data: Vec<_> = data
                .trim()
                .lines()
                .map(|line| line.trim())
                .filter(|line| !line.starts_with('#'))
                .collect();

            if data.is_empty() {
                println!("{}", "Aborted!".red());
                return Ok(());
            }

            let command = data.join("\n");

            if command == *current_command {
                println!("{}", "No changes made, aborting.".dimmed());
                return Ok(());
            }

            match config.get_project_mut(&pwd) {
                Ok(project) => {
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
        }
        Commands::Alias { name } => {
            let mut config = read_config()?;
            config.add_alias(&pwd, &name)?;
            write_config(&config)?;
            println!("Added \"{}\" capabilities in {}", name.blue(), pwd.dimmed());
        }
        Commands::Remove { name } => {
            let mut config = read_config()?;
            let project = config.get_project_mut(&pwd)?;
            match project.remove(&name) {
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
        }
        Commands::Print { json } => {
            let mut config = read_config()?;

            if json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&config.resolve_project(&pwd)?)?
                );
            } else {
                print_project_commands(&config.resolve_project(&pwd)?)
            }
        }
    }

    Ok(())
}

fn build_shell_command(command: &str) -> String {
    format!("{} \"$@\"", command)
}

#[cfg(test)]
mod tests {
    use super::build_shell_command;

    #[test]
    fn appends_forwarded_arguments() {
        assert_eq!(
            build_shell_command("node -e \"console.log(process.argv.slice(1))\""),
            "node -e \"console.log(process.argv.slice(1))\" \"$@\""
        );
    }

    #[test]
    fn preserves_existing_shell_syntax() {
        assert_eq!(
            build_shell_command("FOO=bar npm run dev"),
            "FOO=bar npm run dev \"$@\""
        );
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
        write_config(&Config::default())?;
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
