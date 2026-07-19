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

/// Parser for the user-command path, where the first argument is always one of the user's own
/// commands and never a builtin subcommand.
#[derive(Parser, Debug)]
#[clap(about, version, author)]
struct AliasCli {
    /// The current working directory
    #[clap(long, default_value = ".")]
    pwd: PathBuf,

    /// Print the current command instead of executing it
    #[clap(short, long)]
    print: bool,

    /// The alias to execute
    alias: String,

    /// The arguments to pass to the command
    arguments: Vec<String>,
}

/// The result of pre-scanning the raw arguments, used to decide between the user's own commands
/// and the builtin subcommands before clap gets involved.
#[derive(Debug, Default)]
struct ArgScan {
    /// The value of `--pwd`, when present before a `--` separator
    pwd: Option<String>,

    /// The first token that is not a global flag
    candidate: Option<String>,

    /// The index of the candidate within the scanned slice
    candidate_index: usize,

    /// Whether the candidate appeared after a `--` separator
    escaped: bool,
}

/// Find the first real token (and the `--pwd` value) without parsing the full command line.
fn scan_arguments<S: AsRef<str>>(arguments: &[S]) -> ArgScan {
    let mut scan = ArgScan::default();

    let mut i = 0;
    while i < arguments.len() {
        let token = arguments[i].as_ref();
        match token {
            // Everything after `--` belongs to the command, the first token is always an alias
            "--" => {
                if let Some(next) = arguments.get(i + 1) {
                    scan.candidate = Some(next.as_ref().to_owned());
                    scan.candidate_index = i + 1;
                    scan.escaped = true;
                }
                return scan;
            }
            "--pwd" => {
                if let Some(value) = arguments.get(i + 1) {
                    scan.pwd = Some(value.as_ref().to_owned());
                }
                i += 2;
            }
            "-p" | "--print" => i += 1,
            _ if token.starts_with("--pwd=") => {
                scan.pwd = token.strip_prefix("--pwd=").map(str::to_owned);
                i += 1;
            }
            // Any other flag (`--help`, `--version`, ...) is clap's business
            _ if token.starts_with('-') => return scan,
            _ => {
                scan.candidate = Some(token.to_owned());
                scan.candidate_index = i;

                // Keep scanning for a `--pwd` after the candidate, it decides which project the
                // candidate is resolved in
                let mut j = i + 1;
                while j < arguments.len() {
                    let token = arguments[j].as_ref();
                    if token == "--" {
                        break;
                    } else if token == "--pwd" {
                        if let Some(value) = arguments.get(j + 1) {
                            scan.pwd = Some(value.as_ref().to_owned());
                        }
                        j += 2;
                    } else if let Some(value) = token.strip_prefix("--pwd=") {
                        scan.pwd = Some(value.to_owned());
                        j += 1;
                    } else {
                        j += 1;
                    }
                }

                return scan;
            }
        }
    }

    scan
}

/// The names of all builtin subcommands, including the `taco` namespace itself.
fn builtin_names() -> Vec<String> {
    let mut names: std::collections::BTreeSet<String> = Cli::command()
        .get_subcommands()
        .filter(|command| !command.is_hide_set())
        .map(|command| command.get_name().to_string())
        .collect();

    names.insert("help".to_owned());

    names.into_iter().collect()
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Add a new command
    Add {
        /// The name of the alias for the command to run
        name: String,

        /// The actual command to run
        arguments: Option<Vec<String>>,

        /// Store the command in the `.taco.json` of the current directory instead of your own
        /// config, so it can be committed and shared
        #[clap(long)]
        local: bool,
    },

    /// Edit a command
    Edit {
        /// The name of the alias to edit
        name: String,

        /// Edit the command in the `.taco.json` of the current directory instead of your own
        /// config
        #[clap(long)]
        local: bool,
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

        /// Remove the command from the `.taco.json` of the current directory instead of your own
        /// config
        #[clap(long)]
        local: bool,
    },

    /// Print all the commands
    Print {
        /// Print commands in JSON format
        #[clap(short, long)]
        json: bool,

        /// Show where every command comes from, including shadowed definitions
        #[clap(short, long)]
        verbose: bool,
    },

    /// Open the config file in your editor
    Config {
        /// Open the `.taco.json` of the current directory instead of your own config
        #[clap(long)]
        local: bool,
    },

    /// Check the config for stale projects and dead aliases
    Doctor {
        /// Remove the reported issues from the config
        #[clap(long)]
        fix: bool,
    },

    /// Generate a shell completion script
    Completions {
        /// The shell to generate completions for
        #[clap(value_enum)]
        shell: CompletionShell,
    },

    /// Run a builtin subcommand, even when one of your commands shadows it
    ///
    /// Note: parsed before clap gets involved, this variant only exists so that the `taco`
    /// namespace shows up in the help output.
    Taco,

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

    /// Remove a command from a project. Returns whether the command was removed.
    /// Note: it will not remove commands defined in parent projects.
    fn remove_command(&mut self, project: &Path, name: &str) -> Result<bool> {
        let key = path_key(project)?;
        let Some(commands) = self.projects.get_mut(key) else {
            return Ok(false);
        };

        let removed = commands.remove(name).is_some();

        // Keep the config tidy when the last command of a project is removed
        if commands.is_empty() {
            self.projects.remove(key);
        }

        Ok(removed)
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
    ///
    /// `local` holds the repo-local `.taco.json` commands, keyed by the directory they were found
    /// in (see [`read_local_projects`]). Within the same directory your personal commands and
    /// aliases win over the repo-local ones.
    fn resolve_project_grouped(&self, project: &Path, local: &LocalProjects) -> Vec<CommandGroup> {
        let mut ancestors: Vec<&Path> = project.ancestors().collect();
        ancestors.reverse();

        let mut groups = vec![];
        for ancestor in ancestors {
            let Some(key) = ancestor.to_str() else {
                continue;
            };

            // Repo-local commands, committed alongside the project
            if let Some(commands) = local.get(key) {
                groups.push(CommandGroup {
                    source: format!("{key}/{LOCAL_CONFIG_FILE}"),
                    via: None,
                    local: true,
                    commands: commands.clone(),
                });
            }

            // Commands inherited via aliases
            if let Some(aliases) = self.aliases.get(key) {
                for alias in aliases {
                    if let Some(commands) = self.projects.get(alias) {
                        groups.push(CommandGroup {
                            source: alias.to_owned(),
                            via: Some(key.to_owned()),
                            local: false,
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
                    local: false,
                    commands: commands.clone(),
                });
            }
        }

        groups
    }

    /// Check the config for stale entries.
    fn diagnose(&self) -> Diagnosis {
        let mut diagnosis = Diagnosis::default();

        // Path-based projects whose directory no longer exists. Keys not starting with `/` are
        // named presets and only exist in the config.
        for key in self.projects.keys() {
            if key.starts_with('/') && !Path::new(key).is_dir() {
                diagnosis.missing_projects.push(key.clone());
            }
        }

        for (path, targets) in &self.aliases {
            if !Path::new(path).is_dir() {
                // Reporting this row's targets as well would be noise, the whole row is dead
                diagnosis.dead_alias_paths.push(path.clone());
                continue;
            }

            for target in targets {
                if !self.projects.contains_key(target) {
                    diagnosis
                        .unknown_targets
                        .push((path.clone(), target.clone()));
                }
            }
        }

        // Named presets only contribute commands when they are aliased somewhere
        let referenced: std::collections::BTreeSet<&String> =
            self.aliases.values().flatten().collect();
        for key in self.projects.keys() {
            if !key.starts_with('/') && !referenced.contains(key) {
                diagnosis.unused_presets.push(key.clone());
            }
        }

        // Commands that shadow a builtin subcommand
        let builtins = builtin_names();
        for (project, commands) in &self.projects {
            for name in commands.keys() {
                if builtins.contains(name) {
                    diagnosis
                        .shadowed_builtins
                        .push((project.clone(), name.clone()));
                }
            }
        }

        diagnosis
    }

    /// Get the resolved commands, these are the commands of the current project, merged with all
    /// the parent projects. Deeper projects win over their parents.
    fn resolve_project(&self, project: &Path, local: &LocalProjects) -> Project {
        let mut commands = Project::new();
        for group in self.resolve_project_grouped(project, local) {
            commands.extend(group.commands);
        }

        commands
    }
}

/// Repo-local commands, keyed by the directory their `.taco.json` was found in.
type LocalProjects = BTreeMap<String, Project>;

/// The name of the repo-local config file, committed alongside a project to share its commands.
const LOCAL_CONFIG_FILE: &str = ".taco.json";

/// A repo-local `.taco.json` file. The commands are scoped under a key on purpose, so that the
/// format can grow (a version field, ...) without breaking existing files. Unknown keys are
/// rejected so that files written for a future format fail loudly instead of being ignored.
#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct LocalConfig {
    /// The commands of the project, as a `{"name": "command"}` map
    #[serde(default)]
    commands: Project,
}

/// Read the `.taco.json` of a single directory, `Ok(None)` when the directory has none.
fn read_local_config(dir: &Path) -> Result<Option<LocalConfig>> {
    let file_path = dir.join(LOCAL_CONFIG_FILE);
    match fs::read(&file_path) {
        Ok(contents) => serde_json::from_slice(&contents)
            .map(Some)
            .wrap_err_with(|| format!("Invalid config file: {}", file_path.display())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => {
            Err(e).wrap_err_with(|| format!("Could not read config file: {}", file_path.display()))
        }
    }
}

fn write_local_config(dir: &Path, config: &LocalConfig) -> Result<()> {
    // The trailing newline keeps the committed file friendly to git and other tools
    let contents = format!("{}\n", serde_json::to_string_pretty(config)?);
    write_raw(&dir.join(LOCAL_CONFIG_FILE), contents.as_bytes())
}

/// Find the repo-local `.taco.json` files between the root of the filesystem and `pwd`.
fn read_local_projects(pwd: &Path) -> Result<LocalProjects> {
    let mut local = LocalProjects::new();
    for ancestor in pwd.ancestors() {
        let Some(key) = ancestor.to_str() else {
            continue;
        };

        if let Some(config) = read_local_config(ancestor)? {
            local.insert(key.to_owned(), config.commands);
        }
    }

    Ok(local)
}

/// The issues `taco doctor` found in the config.
#[derive(Debug, Default)]
struct Diagnosis {
    /// Path-based projects whose directory no longer exists
    missing_projects: Vec<String>,

    /// Alias rows attached to a directory that no longer exists
    dead_alias_paths: Vec<String>,

    /// Aliases pointing to a project that is not defined, as `(attachment path, target)` pairs
    unknown_targets: Vec<(String, String)>,

    /// Named presets that are never aliased
    unused_presets: Vec<String>,

    /// Commands that shadow a builtin subcommand, as `(project, name)` pairs
    shadowed_builtins: Vec<(String, String)>,
}

/// A group of commands coming from a single source: a project, or another project inherited via an
/// alias.
#[derive(Debug)]
struct CommandGroup {
    /// The project the commands are defined in
    source: String,

    /// The project that pulled these commands in via an alias, if any
    via: Option<String>,

    /// Whether the commands come from a repo-local `.taco.json` file instead of your own config
    local: bool,

    /// The commands defined in the source project
    commands: Project,
}

fn main() -> Result<()> {
    color_eyre::install()?;

    let argv: Vec<String> = std::env::args().collect();
    let scan = scan_arguments(&argv[1..]);

    // `taco taco {subcommand}` always reaches the builtin subcommands
    if !scan.escaped && scan.candidate.as_deref() == Some("taco") {
        let mut argv = argv.clone();
        argv.remove(scan.candidate_index + 1);

        let args = Cli::parse_from(argv);
        let pwd = canonicalize_pwd(&args.pwd)?;

        let Some(command) = args.command else {
            if let Some(alias) = args.alias {
                println!("`{}` is not a builtin taco command.\n", alias.blue());
                let builtins = builtin_names();
                print_did_you_mean(
                    "taco taco ",
                    &did_you_mean(&alias, builtins.iter().map(String::as_str)),
                );
                std::process::exit(1);
            }

            Cli::command().print_help()?;
            return Ok(());
        };

        return run_builtin(command, pwd);
    }

    // Your own commands always win over the builtin subcommands. An unreadable config (or
    // `.taco.json`) falls through, so that the builtins (like `taco config` to fix it) stay
    // reachable; the fallthrough reports the broken file when the candidate is not a builtin.
    if let Some(candidate) = scan.candidate.as_deref()
        && candidate != "__complete"
        && let Ok(config) = read_config()
    {
        let pwd = canonicalize_pwd(Path::new(scan.pwd.as_deref().unwrap_or(".")))?;
        let project = read_local_projects(&pwd)
            .map(|local| config.resolve_project(&pwd, &local))
            .unwrap_or_default();

        if let Some(command) = project.get(candidate) {
            let args = AliasCli::parse();
            if args.print {
                println!("{command}");
            } else {
                run_command(command, &pwd, &args.arguments)?;
            }

            return Ok(());
        }
    }

    let args = Cli::parse();
    let pwd = canonicalize_pwd(&args.pwd)?;

    let Some(command) = args.command else {
        let Some(alias) = args.alias else {
            Cli::command().print_help()?;
            return Ok(());
        };

        // The command does not exist — it would have been executed above otherwise. A broken
        // config or `.taco.json` also ends up here, and gets reported through the `?`.
        let config = read_config()?;
        let local = read_local_projects(&pwd)?;
        let project = config.resolve_project(&pwd, &local);

        println!("Command `{}` does not exist.\n", alias.blue());
        let builtins = builtin_names();
        let candidates: std::collections::BTreeSet<&str> = project
            .keys()
            .map(String::as_str)
            .chain(builtins.iter().map(String::as_str))
            .collect();
        if !print_did_you_mean("taco ", &did_you_mean(&alias, candidates)) {
            print_flat_commands(&config.resolve_project_grouped(&pwd, &local));
        }
        std::process::exit(1);
    };

    run_builtin(command, pwd)
}

fn canonicalize_pwd(pwd: &Path) -> Result<PathBuf> {
    fs::canonicalize(pwd).wrap_err_with(|| format!("Invalid working directory: {}", pwd.display()))
}

fn run_builtin(command: Commands, pwd: PathBuf) -> Result<()> {
    match command {
        // Only reachable through degenerate nesting like `taco taco taco`; the pre-scan in main
        // handles the real `taco taco {subcommand}` invocations
        Commands::Taco => {
            Cli::command().print_help()?;
        }
        Commands::Add {
            name,
            arguments,
            local,
        } => {
            if name == "taco" || name == "__complete" {
                println!(
                    "{}",
                    format!("\"{name}\" is reserved and cannot be used as a command name.").red()
                );
                std::process::exit(1);
            }

            // Read both stores up front, so a broken file is caught before the editor opens
            let mut config = read_config()?;
            let mut local_config = if local {
                read_local_config(&pwd)?.unwrap_or_default()
            } else {
                LocalConfig::default()
            };

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

            let existing = if local {
                local_config.commands.get(&name).cloned()
            } else {
                config
                    .projects
                    .get(path_key(&pwd)?)
                    .and_then(|project| project.get(&name))
                    .cloned()
            };

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

            let location = if local {
                local_config.commands.insert(name.clone(), command.clone());
                write_local_config(&pwd, &local_config)?;
                pwd.join(LOCAL_CONFIG_FILE).display().to_string()
            } else {
                config.set_command(&pwd, &name, &command)?;
                write_config(&config)?;
                pwd.display().to_string()
            };

            println!(
                "Aliased \"{}\" to \"{}\" in {}",
                name.blue(),
                command.blue(),
                location.dimmed()
            );

            // Your own commands win over `.taco.json` commands in the same directory
            if local
                && config
                    .projects
                    .get(path_key(&pwd)?)
                    .is_some_and(|project| project.contains_key(&name))
            {
                println!(
                    "{}",
                    format!(
                        "Note: your own \"{name}\" command wins over this one in this directory. Run `taco rm {name}` to remove yours."
                    )
                    .dimmed()
                );
            }

            if builtin_names().contains(&name) {
                println!(
                    "{}",
                    format!(
                        "Note: \"{name}\" shadows the builtin `taco {name}`. Reach the builtin with `taco taco {name}`."
                    )
                    .dimmed()
                );
            }
        }
        Commands::Edit { name, local } => {
            if local {
                let mut local_config = read_local_config(&pwd)?.unwrap_or_default();

                let Some(current_command) = local_config.commands.get(&name).cloned() else {
                    println!(
                        "{}\n",
                        format!(
                            "Command \"{name}\" is not defined in {}, cannot edit it.",
                            pwd.join(LOCAL_CONFIG_FILE).display()
                        )
                        .red()
                    );
                    let suggestions =
                        did_you_mean(&name, local_config.commands.keys().map(String::as_str));
                    print_did_you_mean("taco edit --local ", &suggestions);
                    return Ok(());
                };

                let Some(command) = edit_command(Some(&current_command)) else {
                    println!("{}", "Aborted!".red());
                    return Ok(());
                };

                if command == current_command {
                    println!("{}", "No changes made, aborting.".dimmed());
                    return Ok(());
                }

                local_config.commands.insert(name.clone(), command.clone());
                write_local_config(&pwd, &local_config)?;

                println!(
                    "Aliased \"{}\" to \"{}\" in {}",
                    name.blue(),
                    command.blue(),
                    pwd.join(LOCAL_CONFIG_FILE).display().to_string().dimmed()
                );
                return Ok(());
            }

            let mut config = read_config()?;

            let combined_project = config.resolve_project(&pwd, &read_local_projects(&pwd)?);
            let Some(current_command) = combined_project.get(&name) else {
                println!(
                    "{}\n",
                    format!("Command \"{name}\" does not exist, cannot edit it.").red()
                );
                let suggestions = did_you_mean(&name, combined_project.keys().map(String::as_str));
                print_did_you_mean("taco edit ", &suggestions);
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
            let groups = config.resolve_project_grouped(&pwd, &read_local_projects(&pwd)?);

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
                let names: std::collections::BTreeSet<&str> = groups
                    .iter()
                    .flat_map(|group| group.commands.keys())
                    .map(String::as_str)
                    .collect();
                if !print_did_you_mean("taco which ", &did_you_mean(&name, names)) {
                    print_flat_commands(&groups);
                }
                std::process::exit(1);
            }

            let winner = definitions.remove(0);

            println!("taco {}", name.blue());
            for line in winner.commands[&name].lines() {
                println!("  {}", line.dimmed());
            }
            println!("\nDefined in {}", format_group_source(winner));

            if builtin_names().contains(&name) {
                println!(
                    "{}",
                    format!(
                        "This shadows the builtin `taco {name}`. Reach the builtin with `taco taco {name}`."
                    )
                    .dimmed()
                );
            }

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

            if let Some(ancestor) = attached {
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
            } else {
                println!("\"{}\" is not aliased in {}.\n", name.blue(), pwd.display());
                let attached: std::collections::BTreeSet<&str> = pwd
                    .ancestors()
                    .filter_map(|ancestor| ancestor.to_str())
                    .filter_map(|key| config.aliases.get(key))
                    .flatten()
                    .map(String::as_str)
                    .collect();
                print_did_you_mean("taco unalias ", &did_you_mean(&name, attached));
            }
            std::process::exit(1);
        }
        Commands::Remove { name, local } => {
            if local {
                let file_path = pwd.join(LOCAL_CONFIG_FILE);
                let mut local_config = read_local_config(&pwd)?.unwrap_or_default();

                if local_config.commands.remove(&name).is_some() {
                    if local_config.commands.is_empty() {
                        // Keep the repository tidy when the last command is removed
                        fs::remove_file(&file_path)?;
                        println!(
                            "Removed alias \"{}\", and the now empty {}",
                            name.blue(),
                            file_path.display().to_string().dimmed()
                        );
                    } else {
                        write_local_config(&pwd, &local_config)?;
                        println!(
                            "Removed alias \"{}\" from {}",
                            name.blue(),
                            file_path.display().to_string().dimmed()
                        );
                    }
                    return Ok(());
                }

                println!(
                    "Alias \"{}\" is not defined in {}.\n",
                    name.blue(),
                    file_path.display()
                );
                let names = local_config.commands.keys().map(String::as_str);
                print_did_you_mean("taco rm --local ", &did_you_mean(&name, names));
                std::process::exit(1);
            }

            let mut config = read_config()?;

            if config.remove_command(&pwd, &name)? {
                write_config(&config)?;
                println!("Removed alias \"{}\"", name.blue());
                return Ok(());
            }

            // The command might be inherited from a parent project, an alias, or a `.taco.json`
            let groups = config.resolve_project_grouped(&pwd, &read_local_projects(&pwd)?);
            if let Some(group) = groups
                .iter()
                .rev()
                .find(|group| group.commands.contains_key(&name))
            {
                println!(
                    "Alias \"{}\" is not defined in {}, but in {}.",
                    name.blue(),
                    pwd.display(),
                    format_group_source(group)
                );
                if group.local {
                    println!("Edit {} to remove it there.", group.source.blue());
                } else if group.source.starts_with('/') {
                    println!(
                        "Run {} to remove it there.",
                        format!("taco rm {} --pwd {}", name, group.source).blue()
                    );
                } else {
                    println!(
                        "Run {} to edit the \"{}\" project.",
                        "taco config".blue(),
                        group.source.blue()
                    );
                }
            } else {
                println!("Alias \"{}\" does not exist.\n", name.blue());
                let names: std::collections::BTreeSet<&str> = groups
                    .iter()
                    .flat_map(|group| group.commands.keys())
                    .map(String::as_str)
                    .collect();
                if !print_did_you_mean("taco rm ", &did_you_mean(&name, names)) {
                    print_flat_commands(&groups);
                }
            }
            std::process::exit(1);
        }
        Commands::Print { json, verbose } => {
            let config = read_config()?;
            let local = read_local_projects(&pwd)?;

            if json {
                let project = config.resolve_project(&pwd, &local);
                println!("{}", serde_json::to_string_pretty(&project)?);
            } else if verbose {
                print_grouped_commands(&config.resolve_project_grouped(&pwd, &local));
            } else {
                print_flat_commands(&config.resolve_project_grouped(&pwd, &local));
            }
        }
        Commands::Config { local } => {
            let file_path = if local {
                pwd.join(LOCAL_CONFIG_FILE)
            } else {
                config_file_location()?
            };

            // Make sure the file exists, so that the editor has something to open
            if !file_path.exists() {
                if local {
                    write_local_config(&pwd, &LocalConfig::default())?;
                } else {
                    write_config(&Config::default())?;
                }
            }

            let editor = std::env::var("VISUAL")
                .or_else(|_| std::env::var("EDITOR"))
                .map_err(|_| eyre!("No editor configured, set $VISUAL or $EDITOR"))?;

            // The editor may include arguments, e.g. `code --wait`
            let mut parts = editor.split_whitespace();
            let program = parts
                .next()
                .ok_or_else(|| eyre!("No editor configured, set $VISUAL or $EDITOR"))?;
            let editor_args = parts.collect::<Vec<_>>();

            // Snapshot the pre-edit contents, so a broken edit can be rolled back. Restoring is
            // only worth offering when the snapshot itself is valid, e.g. not when `taco config`
            // is being used to repair an already broken file.
            let snapshot = fs::read(&file_path)
                .wrap_err_with(|| format!("Could not read config file: {}", file_path.display()))?;
            let restorable = if local {
                serde_json::from_slice::<LocalConfig>(&snapshot).is_ok()
            } else {
                serde_json::from_slice::<Config>(&snapshot).is_ok()
            };

            // Catches mistakes immediately instead of at the next taco invocation
            let validate = || {
                if local {
                    read_local_config(&pwd).map(|_| ())
                } else {
                    read_config().map(|_| ())
                }
            };

            'edit: loop {
                let status = Command::new(program)
                    .args(&editor_args)
                    .arg(&file_path)
                    .stdin(Stdio::inherit())
                    .stdout(Stdio::inherit())
                    .stderr(Stdio::inherit())
                    .status()
                    .wrap_err_with(|| format!("Failed to open editor: {editor}"))?;

                if !status.success() {
                    return Err(eyre!("Editor exited with a non-zero status"));
                }

                let Err(e) = validate() else { break };
                println!("{}", format!("{e:#}").red());

                loop {
                    let choices = if restorable {
                        "(e)dit again / (r)estore previous config / (k)eep it broken"
                    } else {
                        "(e)dit again / (k)eep it broken"
                    };
                    print!("\nWhat now? {} ", choices.dimmed());
                    let _ = std::io::stdout().flush();

                    let mut answer = String::new();
                    // EOF, e.g. when stdin is not interactive: the config is still broken, so
                    // all that's left is reporting the failure
                    if std::io::stdin().read_line(&mut answer).unwrap_or(0) == 0 {
                        println!();
                        std::process::exit(1);
                    }

                    match answer.trim().to_ascii_lowercase().as_str() {
                        "" | "e" | "edit" => continue 'edit,
                        "r" | "restore" if restorable => {
                            write_raw(&file_path, &snapshot)?;
                            println!("Restored the previous config");
                            break 'edit;
                        }
                        "k" | "keep" => std::process::exit(1),
                        _ => {}
                    }
                }
            }
        }
        Commands::Doctor { fix } => {
            let mut config = read_config()?;
            let file_path = config_file_location()?;
            println!("Checking {}\n", file_path.display().to_string().dimmed());

            let diagnosis = config.diagnose();
            let mut issues = 0;
            let mut fixed = 0;

            if !diagnosis.missing_projects.is_empty() {
                issues += diagnosis.missing_projects.len();
                println!("Project directories that no longer exist:");
                for key in &diagnosis.missing_projects {
                    let commands = config.projects[key].len();
                    println!(
                        "  \u{2219} {} {}",
                        key,
                        format!(
                            "({commands} command{})",
                            if commands == 1 { "" } else { "s" }
                        )
                        .dimmed()
                    );
                }
                if fix && confirm("Remove these projects from the config?") {
                    for key in &diagnosis.missing_projects {
                        config.projects.remove(key);
                    }
                    fixed += diagnosis.missing_projects.len();
                }
                println!();
            }

            if !diagnosis.dead_alias_paths.is_empty() {
                issues += diagnosis.dead_alias_paths.len();
                println!("Aliases attached to directories that no longer exist:");
                for path in &diagnosis.dead_alias_paths {
                    println!(
                        "  \u{2219} {} {}",
                        path,
                        format!("(\u{2192} {})", config.aliases[path].join(", ")).dimmed()
                    );
                }
                if fix && confirm("Remove these aliases from the config?") {
                    for path in &diagnosis.dead_alias_paths {
                        config.aliases.remove(path);
                    }
                    fixed += diagnosis.dead_alias_paths.len();
                }
                println!();
            }

            if !diagnosis.unknown_targets.is_empty() {
                issues += diagnosis.unknown_targets.len();
                println!("Aliases pointing to projects that are not defined:");
                for (path, target) in &diagnosis.unknown_targets {
                    println!(
                        "  \u{2219} {} {}",
                        target.blue(),
                        format!("(aliased in {path})").dimmed()
                    );
                }
                if fix && confirm("Remove these aliases from the config?") {
                    for (path, target) in &diagnosis.unknown_targets {
                        if let Some(aliases) = config.aliases.get_mut(path) {
                            aliases.retain(|alias| alias != target);
                            if aliases.is_empty() {
                                config.aliases.remove(path);
                            }
                        }
                    }
                    fixed += diagnosis.unknown_targets.len();
                }
                println!();
            }

            // Informational only: a preset can be intentionally kept around to alias later, so
            // these are never removed automatically.
            if !diagnosis.unused_presets.is_empty() {
                println!("Presets that are never aliased:");
                for name in &diagnosis.unused_presets {
                    let commands = config.projects[name].len();
                    println!(
                        "  \u{2219} {} {}",
                        name.blue(),
                        format!(
                            "({commands} command{})",
                            if commands == 1 { "" } else { "s" }
                        )
                        .dimmed()
                    );
                }
                println!(
                    "{}",
                    "  These are left untouched, remove them via `taco config` if they are no longer needed.\n"
                        .dimmed()
                );
            }

            // Informational only: shadowing is a feature, your commands win on purpose
            if !diagnosis.shadowed_builtins.is_empty() {
                println!("Commands shadowing a builtin subcommand:");
                for (project, name) in &diagnosis.shadowed_builtins {
                    println!(
                        "  \u{2219} {} {}",
                        name.blue(),
                        format!("(defined in {project})").dimmed()
                    );
                }
                println!(
                    "{}",
                    "  These win over the builtins. Reach the builtins with `taco taco <subcommand>`.\n"
                        .dimmed()
                );
            }

            if fixed > 0 {
                write_config(&config)?;
            }

            let remaining = issues - fixed;
            let plural = |count: usize| if count == 1 { "issue" } else { "issues" };
            match (remaining, fixed) {
                (0, 0) => println!("{}", "No issues found, your taco is fresh!".green()),
                (0, fixed) => println!("{}", format!("Fixed {fixed} {}.", plural(fixed)).green()),
                (remaining, _) if fix => {
                    println!(
                        "{}",
                        format!("{remaining} {} left.", plural(remaining)).red()
                    );
                    std::process::exit(1);
                }
                (remaining, _) => {
                    println!(
                        "{}",
                        format!(
                            "{remaining} {} found. Run `taco doctor --fix` to clean up.",
                            plural(remaining)
                        )
                        .red()
                    );
                    std::process::exit(1);
                }
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
                CompleteKind::Commands => print_completion_pairs(
                    &config.resolve_project(&pwd, &read_local_projects(&pwd)?),
                ),
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

/// Print the resolved commands as a flat list: only the winning definition of each command, in
/// alphabetical order. `print_grouped_commands` is the verbose variant showing where every
/// command comes from.
fn print_flat_commands(groups: &[CommandGroup]) {
    println!("Available commands:\n");

    let builtins = builtin_names();

    // The winning definition of each command: the last group that defines it
    let mut commands: BTreeMap<&String, &String> = BTreeMap::new();
    for group in groups {
        for (name, command) in &group.commands {
            commands.insert(name, command);
        }
    }

    // No commands
    let total = commands.len();
    if total == 0 {
        println!("{}", " \u{2219} There are no commands available.".red());
    }

    // Align the commands in a single column after the longest name
    let width = commands
        .keys()
        .map(|name| name.chars().count())
        .max()
        .unwrap_or(0);

    for (name, command) in &commands {
        // Pad manually: format-width padding would count the invisible color codes
        let padding = " ".repeat(width - name.chars().count());
        let tag = if builtins.contains(name) {
            " (shadows the builtin)".dimmed().to_string()
        } else {
            String::new()
        };

        let mut lines = command.lines();
        let first = lines.next().unwrap_or_default();
        println!("taco {}{padding}  {}{tag}", name.blue(), first.dimmed());
        for line in lines {
            println!("     {}  {}", " ".repeat(width), line.dimmed());
        }
    }

    // Footer
    println!(
        "\n{}",
        format!("{} command{}", total, if total == 1 { "" } else { "s" }).dimmed()
    );
}

/// Print the resolved commands as a tree that mirrors the resolution order: the current project
/// first, with every following source nested one level deeper. The first definition of a command
/// wins; definitions that lost are dimmed and tagged as shadowed.
fn print_grouped_commands(groups: &[CommandGroup]) {
    println!("Available commands:\n");

    let builtins = builtin_names();

    // Winning sources come first: the reverse of the root-first resolution order
    let display: Vec<&CommandGroup> = groups
        .iter()
        .rev()
        .filter(|group| !group.commands.is_empty())
        .collect();

    // The group that wins each command: the first group that defines it
    let mut winner: BTreeMap<&str, usize> = BTreeMap::new();
    for (index, group) in display.iter().enumerate() {
        for name in group.commands.keys() {
            winner.entry(name).or_insert(index);
        }
    }

    // No commands
    let total = winner.len();
    if total == 0 {
        println!("{}", " \u{2219} There are no commands available.".red());
    }

    let mut indent = String::new();
    for (index, group) in display.iter().enumerate() {
        let last_group = index + 1 == display.len();

        if index == 0 {
            println!("{}", format_group_source(group));
        } else {
            println!("{indent}{}", "│".dimmed());
            println!("{indent}{} {}", "└─".dimmed(), format_group_source(group));
            indent.push_str("   ");
        }

        for (position, (name, command)) in group.commands.iter().enumerate() {
            let last = last_group && position + 1 == group.commands.len();
            let (branch, continuation) = if last {
                ("└─", "  ")
            } else {
                ("├─", "│ ")
            };

            let (styled_name, tag) = if winner[name.as_str()] != index {
                (
                    name.dimmed().to_string(),
                    " (shadowed)".dimmed().to_string(),
                )
            } else if builtins.contains(name) {
                (
                    name.blue().to_string(),
                    " (shadows the builtin)".dimmed().to_string(),
                )
            } else {
                (name.blue().to_string(), String::new())
            };

            println!("{indent}{} taco {styled_name}{tag}", branch.dimmed());
            for line in command.lines() {
                println!("{indent}{} {}", continuation.dimmed(), line.dimmed());
            }
        }
    }

    // Footer
    println!(
        "\n{}",
        format!("{} command{}", total, if total == 1 { "" } else { "s" }).dimmed()
    );
}

/// Print `name<TAB>description` pairs for consumption by the shell completion scripts.
fn print_completion_pairs(project: &Project) {
    for (name, command) in project {
        println!("{name}\t{}", command.lines().next().unwrap_or_default());
    }
}

/// Optimal string alignment distance (Levenshtein + transpositions), used for the
/// `Did you mean` suggestions.
fn edit_distance(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();

    let mut matrix = vec![vec![0usize; b.len() + 1]; a.len() + 1];
    for (i, row) in matrix.iter_mut().enumerate() {
        row[0] = i;
    }
    for (j, cell) in matrix[0].iter_mut().enumerate() {
        *cell = j;
    }

    for i in 1..=a.len() {
        for j in 1..=b.len() {
            let cost = usize::from(a[i - 1] != b[j - 1]);
            matrix[i][j] = (matrix[i - 1][j] + 1)
                .min(matrix[i][j - 1] + 1)
                .min(matrix[i - 1][j - 1] + cost);

            // Transpositions like `tets` → `test` count as a single edit
            if i > 1 && j > 1 && a[i - 1] == b[j - 2] && a[i - 2] == b[j - 1] {
                matrix[i][j] = matrix[i][j].min(matrix[i - 2][j - 2] + 1);
            }
        }
    }

    matrix[a.len()][b.len()]
}

/// Find the closest matches to `input` for a `Did you mean` hint, best match first.
fn did_you_mean<'a>(input: &str, candidates: impl IntoIterator<Item = &'a str>) -> Vec<&'a str> {
    let input = input.to_lowercase();
    let threshold = input.chars().count() / 4 + 1;

    let mut scored: Vec<(usize, &str)> = candidates
        .into_iter()
        .map(|candidate| (edit_distance(&input, &candidate.to_lowercase()), candidate))
        .filter(|(distance, _)| *distance <= threshold)
        .collect();

    scored.sort_unstable();
    scored.truncate(3);
    scored.into_iter().map(|(_, candidate)| candidate).collect()
}

/// Print a `Did you mean` hint. Returns whether anything was suggested.
fn print_did_you_mean(prefix: &str, suggestions: &[&str]) -> bool {
    match suggestions {
        [] => false,
        [suggestion] => {
            println!("Did you mean {}?", format!("{prefix}{suggestion}").blue());
            true
        }
        suggestions => {
            println!("Did you mean one of these?\n");
            for suggestion in suggestions {
                println!("  \u{2219} {}", format!("{prefix}{suggestion}").blue());
            }
            true
        }
    }
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
    // Allow overriding the location, mainly useful for (integration) testing
    if let Some(path) = std::env::var_os("TACO_CONFIG")
        && !path.is_empty()
    {
        return Ok(PathBuf::from(path));
    }

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
    write_raw(
        &config_file_location()?,
        serde_json::to_string_pretty(config)?.as_bytes(),
    )
}

fn write_raw(file_path: &Path, contents: &[u8]) -> Result<()> {
    if let Some(parent) = file_path.parent() {
        fs::create_dir_all(parent)?;
    }

    // Write to a temporary file first so a crash mid-write can't corrupt the config.
    let tmp_path = file_path.with_extension("json.tmp");
    fs::write(&tmp_path, contents)?;
    fs::rename(&tmp_path, file_path)?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        Config, LocalProjects, Project, build_shell_command, clean_edited_command, did_you_mean,
        edit_distance, scan_arguments,
    };
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

        let resolved =
            config.resolve_project(Path::new("/projects/app/src"), &LocalProjects::new());
        assert_eq!(resolved.get("test").map(String::as_str), Some("vitest"));
    }

    #[test]
    fn personal_commands_win_over_local_commands_in_the_same_directory() {
        let mut config = Config::default();
        config
            .set_command(Path::new("/projects/app"), "test", "vitest")
            .unwrap();

        let local = LocalProjects::from([(
            "/projects/app".to_owned(),
            Project::from([
                ("test".to_owned(), "jest".to_owned()),
                ("build".to_owned(), "make".to_owned()),
            ]),
        )]);

        let resolved = config.resolve_project(Path::new("/projects/app"), &local);
        assert_eq!(resolved.get("test").map(String::as_str), Some("vitest"));
        assert_eq!(resolved.get("build").map(String::as_str), Some("make"));
    }

    #[test]
    fn deeper_local_commands_win_over_personal_commands_of_a_parent() {
        let mut config = Config::default();
        config
            .set_command(Path::new("/projects"), "test", "jest")
            .unwrap();

        let local = LocalProjects::from([(
            "/projects/app".to_owned(),
            Project::from([("test".to_owned(), "vitest".to_owned())]),
        )]);

        let resolved = config.resolve_project(Path::new("/projects/app"), &local);
        assert_eq!(resolved.get("test").map(String::as_str), Some("vitest"));
    }

    #[test]
    fn local_commands_are_grouped_under_their_file() {
        let local = LocalProjects::from([(
            "/projects/app".to_owned(),
            Project::from([("test".to_owned(), "vitest".to_owned())]),
        )]);

        let groups = Config::default().resolve_project_grouped(Path::new("/projects/app"), &local);
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].source, "/projects/app/.taco.json");
        assert!(groups[0].local);
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

        let groups =
            config.resolve_project_grouped(Path::new("/projects/app"), &LocalProjects::new());
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

        let groups =
            config.resolve_project_grouped(Path::new("/projects/app"), &LocalProjects::new());
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
    fn remove_command_only_removes_from_the_project_itself() {
        let mut config = Config::default();
        config
            .set_command(Path::new("/projects"), "test", "jest")
            .unwrap();

        assert!(
            !config
                .remove_command(Path::new("/projects/app"), "test")
                .unwrap()
        );
        assert!(
            config
                .remove_command(Path::new("/projects"), "test")
                .unwrap()
        );
    }

    #[test]
    fn removing_the_last_command_cleans_up_the_project_entry() {
        let mut config = Config::default();
        config
            .set_command(Path::new("/projects/app"), "test", "jest")
            .unwrap();

        assert!(
            config
                .remove_command(Path::new("/projects/app"), "test")
                .unwrap()
        );
        assert!(config.projects.is_empty());
    }

    #[test]
    fn doctor_finds_stale_entries() {
        let mut config = Config::default();

        // `/` always exists, the made-up paths never do
        config.set_command(Path::new("/"), "test", "ls").unwrap();
        config
            .set_command(Path::new("/taco-test-does-not-exist"), "test", "ls")
            .unwrap();
        config
            .set_command(Path::new("vitest"), "tdd", "vitest")
            .unwrap();
        config
            .set_command(Path::new("prettier"), "format", "prettier -w .")
            .unwrap();
        config.add_alias(Path::new("/"), "prettier").unwrap();
        config.add_alias(Path::new("/"), "missing-preset").unwrap();
        config
            .add_alias(Path::new("/taco-test-also-does-not-exist"), "prettier")
            .unwrap();

        let diagnosis = config.diagnose();
        assert_eq!(
            diagnosis.missing_projects,
            vec!["/taco-test-does-not-exist"]
        );
        assert_eq!(
            diagnosis.dead_alias_paths,
            vec!["/taco-test-also-does-not-exist"]
        );
        assert_eq!(
            diagnosis.unknown_targets,
            vec![("/".to_string(), "missing-preset".to_string())]
        );
        assert_eq!(diagnosis.unused_presets, vec!["vitest"]);
    }

    #[test]
    fn doctor_finds_nothing_in_a_healthy_config() {
        let mut config = Config::default();
        config.set_command(Path::new("/"), "test", "ls").unwrap();
        config
            .set_command(Path::new("vitest"), "tdd", "vitest")
            .unwrap();
        config.add_alias(Path::new("/"), "vitest").unwrap();

        let diagnosis = config.diagnose();
        assert!(diagnosis.missing_projects.is_empty());
        assert!(diagnosis.dead_alias_paths.is_empty());
        assert!(diagnosis.unknown_targets.is_empty());
        assert!(diagnosis.unused_presets.is_empty());
    }

    #[test]
    fn scan_finds_the_candidate_and_pwd() {
        let scan = scan_arguments(&["test", "--watch"]);
        assert_eq!(scan.candidate.as_deref(), Some("test"));
        assert!(!scan.escaped);

        let scan = scan_arguments(&["--pwd", "/x", "test"]);
        assert_eq!(scan.pwd.as_deref(), Some("/x"));
        assert_eq!(scan.candidate.as_deref(), Some("test"));
        assert_eq!(scan.candidate_index, 2);

        // `--pwd` after the candidate still decides the project
        let scan = scan_arguments(&["test", "--pwd=/x"]);
        assert_eq!(scan.pwd.as_deref(), Some("/x"));

        let scan = scan_arguments(&["-p", "config"]);
        assert_eq!(scan.candidate.as_deref(), Some("config"));
    }

    #[test]
    fn scan_treats_double_dash_as_an_escape() {
        let scan = scan_arguments(&["--", "taco"]);
        assert_eq!(scan.candidate.as_deref(), Some("taco"));
        assert!(scan.escaped);

        // Everything after `--` belongs to the command, including a `--pwd`
        let scan = scan_arguments(&["--", "test", "--pwd", "/x"]);
        assert_eq!(scan.pwd, None);
    }

    #[test]
    fn scan_leaves_other_flags_to_clap() {
        assert_eq!(scan_arguments(&["--help"]).candidate, None);
        assert_eq!(scan_arguments(&["--version"]).candidate, None);
    }

    #[test]
    fn doctor_reports_commands_shadowing_builtins() {
        let mut config = Config::default();
        config
            .set_command(Path::new("/"), "config", "echo custom")
            .unwrap();
        config.set_command(Path::new("/"), "test", "ls").unwrap();

        let diagnosis = config.diagnose();
        assert_eq!(
            diagnosis.shadowed_builtins,
            vec![("/".to_string(), "config".to_string())]
        );
    }

    #[test]
    fn edit_distance_counts_transpositions_as_one_edit() {
        assert_eq!(edit_distance("test", "test"), 0);
        assert_eq!(edit_distance("tets", "test"), 1);
        assert_eq!(edit_distance("biuld", "build"), 1);
        assert_eq!(edit_distance("tst", "test"), 1);
        assert_eq!(edit_distance("dev", "watch"), 5);
    }

    #[test]
    fn did_you_mean_suggests_the_closest_command() {
        assert_eq!(did_you_mean("tets", ["test", "tdd", "build"]), vec!["test"]);
        assert_eq!(
            did_you_mean("biuld", ["test", "tdd", "build"]),
            vec!["build"]
        );
        assert_eq!(did_you_mean("tst", ["tdd", "test"]), vec!["test"]);
        assert!(did_you_mean("deploy", ["test", "tdd", "build"]).is_empty());
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

        let resolved = config.resolve_project(Path::new("/projects/app"), &LocalProjects::new());
        assert_eq!(resolved.get("dev").map(String::as_str), Some("npm run dev"));
    }
}
