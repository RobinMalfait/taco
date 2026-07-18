use clap::{Parser, Subcommand};
use color_eyre::eyre::{Result, WrapErr, eyre};
use colored::*;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::io::{Error, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};
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
    fn add_alias(&mut self, project: &Path, alias: &str) -> Result<()> {
        let key = path_key(project)?;

        self.aliases
            .entry(key.into())
            .or_insert(vec![])
            .push(alias.into());

        Ok(())
    }

    /// Get the current project's commands.
    /// Note: it will not merge the commands with any parent projects.
    fn get_project_mut(&mut self, project: &Path) -> Result<&mut Project> {
        let key = path_key(project)?;
        self.projects
            .get_mut(key)
            .ok_or_else(|| eyre!("Project not found: {}", project.display()))
    }

    /// Get the resolved commands, these are the commands of the current project, merged with all
    /// the parent projects. Deeper projects win over their parents.
    fn resolve_project(&self, project: &Path) -> Project {
        let mut ancestors: Vec<&Path> = project.ancestors().collect();
        ancestors.reverse();

        let mut commands = Project::new();
        for ancestor in ancestors {
            let Some(key) = ancestor.to_str() else {
                continue;
            };

            // Commands inherited via aliases
            if let Some(aliases) = self.aliases.get(key) {
                for alias in aliases {
                    if let Some(project) = self.projects.get(alias) {
                        commands.extend(project.clone());
                    }
                }
            }

            // Commands of the project itself
            if let Some(project) = self.projects.get(key) {
                commands.extend(project.clone());
            }
        }

        commands
    }
}

fn main() -> Result<()> {
    let args = Cli::parse();

    let pwd = fs::canonicalize(&args.pwd)
        .wrap_err_with(|| format!("Invalid working directory: {}", args.pwd.display()))?;

    let Some(command) = args.command else {
        if args.alias.is_none() {
            print_help()?;
        }

        let config = read_config()?;
        let alias = &args.alias.unwrap();
        let print = args.print;
        let arguments = args.arguments;
        let project = config.resolve_project(&pwd);

        match project.get(alias) {
            Some(command) if print => println!("{command}"),
            Some(command) => run_command(command, &pwd, &arguments)?,
            None => {
                // Project exists but command doesn't.
                println!("Command `{}` does not exist.\n", alias.blue());
                print_project_commands(&project);
                std::process::exit(1);
            }
        }

        return Ok(());
    };

    match command {
        Commands::Add { name, arguments } => {
            let mut config = read_config()?;
            let command = match arguments {
                Some(args) => args.join(" "),
                None => match edit_command(None) {
                    Some(command) => command,
                    None => {
                        println!("{}", "Aborted!".red());
                        return Ok(());
                    }
                },
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
                    config.projects.insert(path_key(&pwd)?.to_owned(), project);
                    write_config(&config)?;
                }
            }

            println!(
                "Aliased \"{}\" to \"{}\" in {}",
                name.blue(),
                &command.blue(),
                pwd.display().to_string().dimmed()
            );
        }
        Commands::Edit { name } => {
            let mut config = read_config()?;

            let combined_project = config.resolve_project(&pwd);
            let Some(current_command) = combined_project.get(&name) else {
                println!(
                    "{}",
                    format!("Command \"{}\" does not exist, cannot edit it.", name).red()
                );
                return Ok(());
            };

            let Some(command) = edit_command(Some(current_command)) else {
                println!("{}", "Aborted!".red());
                return Ok(());
            };

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
                    config.projects.insert(path_key(&pwd)?.to_owned(), project);
                    write_config(&config)?;
                }
            }

            println!(
                "Aliased \"{}\" to \"{}\" in {}",
                name.blue(),
                &command.blue(),
                pwd.display().to_string().dimmed()
            );
        }
        Commands::Alias { name } => {
            let mut config = read_config()?;
            config.add_alias(&pwd, &name)?;
            write_config(&config)?;
            println!(
                "Added \"{}\" capabilities in {}",
                name.blue(),
                pwd.display().to_string().dimmed()
            );
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
            let config = read_config()?;
            let project = config.resolve_project(&pwd);

            if json {
                println!("{}", serde_json::to_string_pretty(&project)?);
            } else {
                print_project_commands(&project);
            }
        }
    }

    Ok(())
}

/// Run the aliased command through the user's shell, forwarding any extra arguments, and exit with
/// the same status code as the command itself.
fn run_command(command: &str, pwd: &Path, arguments: &[String]) -> Result<()> {
    let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_string());

    let mut cmd = Command::new(&shell);
    cmd.current_dir(pwd);

    // Interactive mode so that aliases/functions from the shell's rc files are available
    if Path::new(&shell)
        .file_name()
        .is_some_and(|name| name == "zsh")
    {
        cmd.arg("-i");
    }

    let status = cmd
        .arg("-c")
        .arg(build_shell_command(command))
        .arg("taco") // $0 for the command
        .args(arguments)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .wrap_err_with(|| format!("Failed to execute {shell}"))?;

    std::process::exit(exit_code(status));
}

/// Mirror the child's exit code; treat death-by-signal as 128 + signal like most shells do.
fn exit_code(status: ExitStatus) -> i32 {
    if let Some(code) = status.code() {
        return code;
    }

    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        if let Some(signal) = status.signal() {
            return 128 + signal;
        }
    }

    1
}

fn build_shell_command(command: &str) -> String {
    format!("{} \"$@\"", command)
}

/// Open the user's editor to write/edit a command, and clean up the result.
/// Returns `None` when the edit was aborted or ended up empty.
fn edit_command(current_command: Option<&str>) -> Option<String> {
    let template = format!(
        "{}\n# Enter the command you want to alias here.\n# Lines starting with '#' are ignored.\n",
        current_command.unwrap_or("")
    );

    clean_edited_command(&rich_edit(&template)?)
}

fn clean_edited_command(data: &str) -> Option<String> {
    let lines: Vec<_> = data
        .trim()
        .lines()
        .map(str::trim)
        .filter(|line| !line.starts_with('#'))
        .collect();

    if lines.iter().all(|line| line.is_empty()) {
        return None;
    }

    Some(lines.join("\n"))
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

fn path_key(path: &Path) -> Result<&str> {
    path.to_str()
        .ok_or_else(|| eyre!("Path is not valid UTF-8: {}", path.display()))
}

// Currently using a library that automatically gives you the
// config dir, which does all the magic for you (including the $HOME, $XDG_CONFIG_HOME, ...).
// However, I'm on MacOS and I also want to use `~/.config`, but it results in
// `$HOME/Library/Application Support` instead, which sort of makes sense but I don't want that...
// Therefore doing this manually.
fn config_file_location() -> Result<PathBuf> {
    let home = dirs::home_dir().ok_or_else(|| eyre!("Could not determine home directory"))?;
    Ok(home.join(".config").join("taco").join("taco.json"))
}

fn read_config() -> Result<Config> {
    let file_path = config_file_location()?;
    match fs::read(&file_path) {
        Ok(contents) => serde_json::from_slice(&contents)
            .wrap_err_with(|| format!("Invalid config file: {}", file_path.display())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Config::default()),
        Err(e) => {
            Err(e).wrap_err_with(|| format!("Could not read config file: {}", file_path.display()))
        }
    }
}

fn write_config(config: &Config) -> Result<()> {
    let file_path = config_file_location()?;
    if let Some(parent) = file_path.parent() {
        fs::create_dir_all(parent)?;
    }

    // Write to a temporary file first so a crash mid-write can't corrupt the config.
    let tmp_path = file_path.with_extension("json.tmp");
    fs::write(&tmp_path, serde_json::to_string_pretty(config)?)?;
    fs::rename(&tmp_path, &file_path)?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{Config, build_shell_command, clean_edited_command};
    use std::path::Path;

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

    #[test]
    fn strips_comments_and_surrounding_whitespace() {
        assert_eq!(
            clean_edited_command("\nnpm run dev\n# a comment\n"),
            Some("npm run dev".to_string())
        );
    }

    #[test]
    fn keeps_blank_lines_between_commands() {
        assert_eq!(
            clean_edited_command("echo one\n\necho two\n# done\n"),
            Some("echo one\n\necho two".to_string())
        );
    }

    #[test]
    fn rejects_empty_or_comment_only_input() {
        assert_eq!(clean_edited_command(""), None);
        assert_eq!(clean_edited_command("\n \n"), None);
        assert_eq!(clean_edited_command("# only comments\n# here\n"), None);
    }

    #[test]
    fn child_projects_override_parents() {
        let mut config = Config::default();
        config
            .projects
            .entry("/projects".to_string())
            .or_default()
            .insert("test".to_string(), "jest".to_string());
        config
            .projects
            .entry("/projects/app".to_string())
            .or_default()
            .insert("test".to_string(), "vitest".to_string());

        let resolved = config.resolve_project(Path::new("/projects/app/src"));
        assert_eq!(resolved.get("test").map(String::as_str), Some("vitest"));
    }

    #[test]
    fn aliased_projects_are_inherited() {
        let mut config = Config::default();
        config
            .projects
            .entry("/presets/webdev".to_string())
            .or_default()
            .insert("dev".to_string(), "npm run dev".to_string());
        config
            .add_alias(Path::new("/projects/app"), "/presets/webdev")
            .unwrap();

        let resolved = config.resolve_project(Path::new("/projects/app"));
        assert_eq!(resolved.get("dev").map(String::as_str), Some("npm run dev"));
    }
}
