use clap::{CommandFactory, Parser, Subcommand};
use color_eyre::eyre::{Result, WrapErr, eyre};
use colored::Colorize;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
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

    /// Show where a command is defined
    Which {
        /// The name of the alias to look up
        name: String,
    },

    /// Alias the current project to a predefined project
    Alias {
        /// The name of the alias
        name: String,
    },

    /// Remove an alias from the current project
    Unalias {
        /// The name of the alias to remove
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

    /// Open the config file in your editor
    Config,

    /// Generate a shell completion script
    Completions {
        /// The shell to generate completions for
        #[clap(value_enum)]
        shell: CompletionShell,
    },

    /// Complete dynamic values, used by the shell completion scripts
    #[clap(name = "__complete", hide = true)]
    Complete {
        /// The kind of values to complete
        #[clap(value_enum)]
        kind: CompleteKind,
    },
}

#[derive(Clone, Copy, Debug, clap::ValueEnum)]
enum CompletionShell {
    Zsh,
    Bash,
    Fish,
}

#[derive(Clone, Copy, Debug, clap::ValueEnum)]
enum CompleteKind {
    /// All commands available in the current directory, including inherited ones
    Commands,

    /// Only the commands defined in the current directory itself
    LocalCommands,

    /// Projects that can be used as an alias target
    Projects,

    /// Aliases attached to the current directory or one of its parents
    Aliases,
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
    /// Register an alias for a project
    fn add_alias(&mut self, project: &Path, alias: &str) -> Result<()> {
        let key = path_key(project)?;
        let aliases = self.aliases.entry(key.to_owned()).or_default();
        if !aliases.iter().any(|existing| existing == alias) {
            aliases.push(alias.to_owned());
        }

        Ok(())
    }

    /// Remove an alias from a project. Returns whether the alias was removed.
    fn remove_alias(&mut self, project: &Path, alias: &str) -> Result<bool> {
        let key = path_key(project)?;
        let Some(aliases) = self.aliases.get_mut(key) else {
            return Ok(false);
        };

        let before = aliases.len();
        aliases.retain(|existing| existing != alias);
        let removed = aliases.len() != before;

        // Keep the config tidy when the last alias of a project is removed
        if aliases.is_empty() {
            self.aliases.remove(key);
        }

        Ok(removed)
    }

    /// Get the current project's commands.
    /// Note: it will not merge the commands with any parent projects.
    fn get_project_mut(&mut self, project: &Path) -> Result<&mut Project> {
        let key = path_key(project)?;
        self.projects
            .get_mut(key)
            .ok_or_else(|| eyre!("Project not found: {}", project.display()))
    }

    /// Insert (or overwrite) a command in the project, creating the project if needed.
    fn set_command(&mut self, project: &Path, name: &str, command: &str) -> Result<()> {
        let key = path_key(project)?;
        self.projects
            .entry(key.to_owned())
            .or_default()
            .insert(name.to_owned(), command.to_owned());

        Ok(())
    }

    /// Get the resolved commands, grouped by the project they are defined in, ordered from the
    /// root of the filesystem down to the project itself. Groups can contain commands that are
    /// overridden by a later group.
    fn resolve_project_grouped(&self, project: &Path) -> Vec<CommandGroup> {
        let mut ancestors: Vec<&Path> = project.ancestors().collect();
        ancestors.reverse();

        let mut groups = vec![];
        for ancestor in ancestors {
            let Some(key) = ancestor.to_str() else {
                continue;
            };

            // Commands inherited via aliases
            if let Some(aliases) = self.aliases.get(key) {
                for alias in aliases {
                    if let Some(commands) = self.projects.get(alias) {
                        groups.push(CommandGroup {
                            source: alias.to_owned(),
                            via: Some(key.to_owned()),
                            commands: commands.clone(),
                        });
                    }
                }
            }

            // Commands of the project itself
            if let Some(commands) = self.projects.get(key) {
                groups.push(CommandGroup {
                    source: key.to_owned(),
                    via: None,
                    commands: commands.clone(),
                });
            }
        }

        groups
    }

    /// Get the resolved commands, these are the commands of the current project, merged with all
    /// the parent projects. Deeper projects win over their parents.
    fn resolve_project(&self, project: &Path) -> Project {
        let mut commands = Project::new();
        for group in self.resolve_project_grouped(project) {
            commands.extend(group.commands);
        }

        commands
    }
}

/// A group of commands coming from a single source: a project, or another project inherited via an
/// alias.
#[derive(Debug)]
struct CommandGroup {
    /// The project the commands are defined in
    source: String,

    /// The project that pulled these commands in via an alias, if any
    via: Option<String>,

    /// The commands defined in the source project
    commands: Project,
}

fn main() -> Result<()> {
    color_eyre::install()?;
    let args = Cli::parse();

    let pwd = fs::canonicalize(&args.pwd)
        .wrap_err_with(|| format!("Invalid working directory: {}", args.pwd.display()))?;

    let Some(command) = args.command else {
        let Some(alias) = args.alias else {
            Cli::command().print_help()?;
            return Ok(());
        };

        let config = read_config()?;
        let project = config.resolve_project(&pwd);

        match project.get(&alias) {
            Some(command) if args.print => println!("{command}"),
            Some(command) => run_command(command, &pwd, &args.arguments)?,
            None => {
                // Project exists but command doesn't.
                println!("Command `{}` does not exist.\n", alias.blue());
                print_grouped_commands(&config.resolve_project_grouped(&pwd));
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
                None => {
                    if let Some(command) = edit_command(None) {
                        command
                    } else {
                        println!("{}", "Aborted!".red());
                        return Ok(());
                    }
                }
            };

            let existing = config
                .projects
                .get(path_key(&pwd)?)
                .and_then(|project| project.get(&name));

            if let Some(existing) = existing {
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

            config.set_command(&pwd, &name, &command)?;
            write_config(&config)?;

            println!(
                "Aliased \"{}\" to \"{}\" in {}",
                name.blue(),
                command.blue(),
                pwd.display().to_string().dimmed()
            );
        }
        Commands::Edit { name } => {
            let mut config = read_config()?;

            let combined_project = config.resolve_project(&pwd);
            let Some(current_command) = combined_project.get(&name) else {
                println!(
                    "{}",
                    format!("Command \"{name}\" does not exist, cannot edit it.").red()
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

            config.set_command(&pwd, &name, &command)?;
            write_config(&config)?;

            println!(
                "Aliased \"{}\" to \"{}\" in {}",
                name.blue(),
                command.blue(),
                pwd.display().to_string().dimmed()
            );
        }
        Commands::Which { name } => {
            let config = read_config()?;
            let groups = config.resolve_project_grouped(&pwd);

            // Deepest definition first. The same project can be aliased at multiple levels, but
            // its commands are identical, so only the deepest occurrence matters.
            let mut seen = std::collections::BTreeSet::new();
            let mut definitions: Vec<&CommandGroup> = groups
                .iter()
                .rev()
                .filter(|group| group.commands.contains_key(&name))
                .filter(|group| seen.insert(group.source.as_str()))
                .collect();

            if definitions.is_empty() {
                println!("Command `{}` does not exist.\n", name.blue());
                print_grouped_commands(&groups);
                std::process::exit(1);
            }

            let winner = definitions.remove(0);

            println!("taco {}", name.blue());
            for line in winner.commands[&name].lines() {
                println!("  {}", line.dimmed());
            }
            println!("\nDefined in {}", format_group_source(winner));

            // The definitions that lost from the winner, closest one first
            if !definitions.is_empty() {
                println!("\nShadowed definitions:");
                for group in definitions {
                    println!("  {}", format_group_source(group));
                    for line in group.commands[&name].lines() {
                        println!("    {}", line.dimmed());
                    }
                }
            }
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
        Commands::Unalias { name } => {
            let mut config = read_config()?;

            if config.remove_alias(&pwd, &name)? {
                write_config(&config)?;
                println!(
                    "Removed \"{}\" capabilities from {}",
                    name.blue(),
                    pwd.display().to_string().dimmed()
                );
                return Ok(());
            }

            // The alias might be attached to a parent directory instead
            let attached = pwd.ancestors().skip(1).find(|ancestor| {
                ancestor
                    .to_str()
                    .and_then(|key| config.aliases.get(key))
                    .is_some_and(|aliases| aliases.contains(&name))
            });

            match attached {
                Some(ancestor) => {
                    println!(
                        "\"{}\" is not aliased in {}, but in {}.",
                        name.blue(),
                        pwd.display(),
                        ancestor.display()
                    );
                    println!(
                        "Run {} to remove it there.",
                        format!("taco unalias {} --pwd {}", name, ancestor.display()).blue()
                    );
                }
                None => println!("\"{}\" is not aliased in {}.", name.blue(), pwd.display()),
            }
            std::process::exit(1);
        }
        Commands::Remove { name } => {
            let mut config = read_config()?;
            let project = config.get_project_mut(&pwd)?;
            if project.remove(&name).is_some() {
                write_config(&config)?;
                println!("Removed alias \"{}\"\n", name.blue());
            } else {
                println!("Alias \"{}\" does not exist.\n", name.blue());
                print_project_commands(project);
            }
        }
        Commands::Print { json } => {
            let config = read_config()?;

            if json {
                let project = config.resolve_project(&pwd);
                println!("{}", serde_json::to_string_pretty(&project)?);
            } else {
                print_grouped_commands(&config.resolve_project_grouped(&pwd));
            }
        }
        Commands::Config => {
            let file_path = config_file_location()?;

            // Make sure the file exists, so that the editor has something to open
            if !file_path.exists() {
                write_config(&Config::default())?;
            }

            let editor = std::env::var("VISUAL")
                .or_else(|_| std::env::var("EDITOR"))
                .map_err(|_| eyre!("No editor configured, set $VISUAL or $EDITOR"))?;

            // The editor may include arguments, e.g. `code --wait`
            let mut parts = editor.split_whitespace();
            let program = parts
                .next()
                .ok_or_else(|| eyre!("No editor configured, set $VISUAL or $EDITOR"))?;

            let status = Command::new(program)
                .args(parts)
                .arg(&file_path)
                .stdin(Stdio::inherit())
                .stdout(Stdio::inherit())
                .stderr(Stdio::inherit())
                .status()
                .wrap_err_with(|| format!("Failed to open editor: {editor}"))?;

            if !status.success() {
                return Err(eyre!("Editor exited with a non-zero status"));
            }

            // Catch mistakes immediately instead of at the next taco invocation
            if let Err(e) = read_config() {
                println!("{}", format!("{e:#}").red());
                std::process::exit(1);
            }
        }
        Commands::Completions { shell } => {
            let script = match shell {
                CompletionShell::Zsh => include_str!("completions/taco.zsh"),
                CompletionShell::Bash => include_str!("completions/taco.bash"),
                CompletionShell::Fish => include_str!("completions/taco.fish"),
            };
            print!("{script}");
        }
        Commands::Complete { kind } => {
            let config = read_config()?;
            match kind {
                CompleteKind::Commands => print_completion_pairs(&config.resolve_project(&pwd)),
                CompleteKind::LocalCommands => {
                    if let Some(project) = config.projects.get(path_key(&pwd)?) {
                        print_completion_pairs(project);
                    }
                }
                CompleteKind::Projects => {
                    for (name, project) in &config.projects {
                        let commands = project.len();
                        println!(
                            "{name}\t{commands} command{}",
                            if commands == 1 { "" } else { "s" }
                        );
                    }
                }
                CompleteKind::Aliases => {
                    // Deepest attachment first; the same alias can be attached at multiple levels
                    let mut seen = std::collections::BTreeSet::new();
                    for ancestor in pwd.ancestors() {
                        let Some(key) = ancestor.to_str() else {
                            continue;
                        };

                        if let Some(aliases) = config.aliases.get(key) {
                            for alias in aliases {
                                if seen.insert(alias.as_str()) {
                                    println!("{alias}\taliased in {key}");
                                }
                            }
                        }
                    }
                }
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
    format!("{command} \"$@\"")
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

/// Format the source of a command group, e.g. `/path` or `vitest (via alias in /path)`.
fn format_group_source(group: &CommandGroup) -> String {
    match &group.via {
        Some(via) => format!(
            "{} {}",
            group.source.bold(),
            format!("(via alias in {via})").dimmed()
        ),
        None => group.source.bold().to_string(),
    }
}

/// Print the resolved commands as a tree, grouped by the project they are defined in. Commands
/// that are overridden by a deeper project are only shown in the group that won.
fn print_grouped_commands(groups: &[CommandGroup]) {
    println!("Available commands:\n");

    // The group that wins each command: the last group that defines it
    let mut winner: BTreeMap<&str, usize> = BTreeMap::new();
    for (index, group) in groups.iter().enumerate() {
        for name in group.commands.keys() {
            winner.insert(name, index);
        }
    }

    // No commands
    let total = winner.len();
    if total == 0 {
        println!("{}", " \u{2219} There are no commands available.\n".red());
    }

    // Commands
    for (index, group) in groups.iter().enumerate() {
        let commands: Vec<_> = group
            .commands
            .iter()
            .filter(|(name, _)| winner[name.as_str()] == index)
            .collect();

        if commands.is_empty() {
            continue;
        }

        println!("{}", format_group_source(group));
        for (position, (name, command)) in commands.iter().enumerate() {
            let last = position + 1 == commands.len();
            let (branch, continuation) = if last {
                ("└─", "  ")
            } else {
                ("├─", "│ ")
            };

            println!("  {} taco {}", branch.dimmed(), name.blue());
            for line in command.lines() {
                println!("  {} {}", continuation.dimmed(), line.dimmed());
            }
        }
        println!();
    }

    // Footer
    println!(
        "{}",
        format!("{} command{}", total, if total == 1 { "" } else { "s" }).dimmed()
    );
}

/// Print `name<TAB>description` pairs for consumption by the shell completion scripts.
fn print_completion_pairs(project: &Project) {
    for (name, command) in project {
        println!("{name}\t{}", command.lines().next().unwrap_or_default());
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
            if commands == 1 { "" } else { "s" }
        )
        .dimmed()
    );
}

fn confirm(message: &str) -> bool {
    print!("{} {} ", message, "(y/N)".dimmed());
    let _ = std::io::stdout().flush();

    let mut answer = String::new();
    if std::io::stdin().read_line(&mut answer).is_err() {
        return false;
    }

    println!();

    answer.trim().eq_ignore_ascii_case("y")
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
            .set_command(Path::new("/projects"), "test", "jest")
            .unwrap();
        config
            .set_command(Path::new("/projects/app"), "test", "vitest")
            .unwrap();

        let resolved = config.resolve_project(Path::new("/projects/app/src"));
        assert_eq!(resolved.get("test").map(String::as_str), Some("vitest"));
    }

    #[test]
    fn grouped_resolution_orders_parents_first() {
        let mut config = Config::default();
        config
            .set_command(Path::new("/projects"), "test", "jest")
            .unwrap();
        config
            .set_command(Path::new("/projects/app"), "test", "vitest")
            .unwrap();

        let groups = config.resolve_project_grouped(Path::new("/projects/app"));
        let sources: Vec<_> = groups.iter().map(|group| group.source.as_str()).collect();
        assert_eq!(sources, vec!["/projects", "/projects/app"]);
    }

    #[test]
    fn grouped_resolution_tracks_the_aliased_project() {
        let mut config = Config::default();
        config
            .set_command(Path::new("vitest"), "test", "vitest run")
            .unwrap();
        config
            .add_alias(Path::new("/projects/app"), "vitest")
            .unwrap();

        let groups = config.resolve_project_grouped(Path::new("/projects/app"));
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].source, "vitest");
        assert_eq!(groups[0].via.as_deref(), Some("/projects/app"));
    }

    #[test]
    fn remove_alias_removes_a_single_alias() {
        let mut config = Config::default();
        config
            .add_alias(Path::new("/projects/app"), "vitest")
            .unwrap();
        config
            .add_alias(Path::new("/projects/app"), "prettier")
            .unwrap();

        assert!(
            config
                .remove_alias(Path::new("/projects/app"), "vitest")
                .unwrap()
        );
        assert!(
            !config
                .remove_alias(Path::new("/projects/app"), "vitest")
                .unwrap()
        );
        assert_eq!(config.aliases["/projects/app"], vec!["prettier"]);
    }

    #[test]
    fn removing_the_last_alias_cleans_up_the_project_entry() {
        let mut config = Config::default();
        config
            .add_alias(Path::new("/projects/app"), "vitest")
            .unwrap();

        assert!(
            config
                .remove_alias(Path::new("/projects/app"), "vitest")
                .unwrap()
        );
        assert!(config.aliases.is_empty());
    }

    #[test]
    fn aliased_projects_are_inherited() {
        let mut config = Config::default();
        config
            .set_command(Path::new("/presets/webdev"), "dev", "npm run dev")
            .unwrap();
        config
            .add_alias(Path::new("/projects/app"), "/presets/webdev")
            .unwrap();

        let resolved = config.resolve_project(Path::new("/projects/app"));
        assert_eq!(resolved.get("dev").map(String::as_str), Some("npm run dev"));
    }
}
