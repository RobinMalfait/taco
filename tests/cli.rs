use std::fs;
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use insta::assert_snapshot;

/// A sandboxed taco environment: a temporary project directory (with a `nested` subdirectory) and
/// its own config file, wired up through `TACO_CONFIG`.
struct Sandbox {
    // Held on to so the temporary directory outlives the test
    _root: tempfile::TempDir,
    base: PathBuf,
    config: PathBuf,
    project: PathBuf,
}

impl Sandbox {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        // Canonicalize, so that the paths in the config match taco's canonicalized `--pwd`
        let base = root.path().canonicalize().unwrap();
        let project = base.join("project");
        fs::create_dir_all(project.join("nested")).unwrap();

        Sandbox {
            _root: root,
            config: base.join("taco.json"),
            base,
            project,
        }
    }

    fn nested(&self) -> PathBuf {
        self.project.join("nested")
    }

    fn write_config(&self, json: &str) {
        fs::write(&self.config, json).unwrap();
    }

    /// The config file contents, with the sandbox location normalized for snapshots.
    fn config_contents(&self) -> String {
        self.normalize(&fs::read_to_string(&self.config).unwrap())
    }

    /// Run taco in the project directory and render the result for snapshotting.
    fn taco(&self, arguments: &[&str]) -> String {
        self.taco_stdin(&self.project, arguments, "")
    }

    /// Run taco in the given directory and render the result for snapshotting.
    fn taco_in(&self, dir: &Path, arguments: &[&str]) -> String {
        self.taco_stdin(dir, arguments, "")
    }

    /// Run taco with the given stdin (for `(y/N)` confirmation prompts) and render the result for
    /// snapshotting.
    fn taco_stdin(&self, dir: &Path, arguments: &[&str], stdin: &str) -> String {
        let mut command = self.taco_command(dir);
        command.args(arguments);
        self.spawn(command, stdin)
    }

    /// Run taco with the given editor script available as `$EDITOR`.
    fn taco_with_editor(&self, editor: &Path, arguments: &[&str], stdin: &str) -> String {
        let mut command = self.taco_command(&self.project);
        command.args(arguments).env("EDITOR", editor);
        self.spawn(command, stdin)
    }

    /// Write a repo-local `.taco.json` in the project directory.
    fn write_local_config(&self, json: &str) {
        fs::write(self.project.join(".taco.json"), json).unwrap();
    }

    /// The `.taco.json` contents of the project directory, normalized for snapshots.
    fn local_config_contents(&self) -> String {
        self.normalize(&fs::read_to_string(self.project.join(".taco.json")).unwrap())
    }

    /// Write an executable script that can act as `$EDITOR`, receiving the config file as `$1`.
    fn editor(&self, name: &str, body: &str) -> PathBuf {
        let path = self.base.join(name);
        fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
        path
    }

    fn taco_command(&self, dir: &Path) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_taco"));
        command
            .current_dir(dir)
            .env("TACO_CONFIG", &self.config)
            .env("SHELL", "/bin/sh")
            .env("NO_COLOR", "1")
            .env_remove("VISUAL")
            .env_remove("EDITOR")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        command
    }

    fn spawn(&self, mut command: Command, stdin: &str) -> String {
        let mut child = command.spawn().unwrap();
        child
            .stdin
            .as_mut()
            .unwrap()
            .write_all(stdin.as_bytes())
            .unwrap();
        self.render(child.wait_with_output().unwrap())
    }

    /// Render an `Output` as `exit code` + `stdout` + `stderr` sections, with the sandbox
    /// location normalized so that snapshots are deterministic.
    fn render(&self, output: Output) -> String {
        let mut rendered = format!(
            "exit code: {}\n",
            output
                .status
                .code()
                .map_or("killed by signal".to_owned(), |code| code.to_string())
        );

        for (name, contents) in [("stdout", &output.stdout), ("stderr", &output.stderr)] {
            let contents = String::from_utf8_lossy(contents);
            if !contents.trim().is_empty() {
                rendered.push_str(&format!("----- {name} -----\n{contents}"));
            }
        }

        self.normalize(&rendered)
    }

    fn normalize(&self, contents: &str) -> String {
        contents.replace(&self.base.display().to_string(), "<root>")
    }
}

#[test]
fn add_and_run_a_command() {
    let sandbox = Sandbox::new();

    assert_snapshot!(
        "add_a_command",
        sandbox.taco(&["add", "greet", "--", "echo", "hello"])
    );
    assert_snapshot!("run_a_command", sandbox.taco(&["greet"]));
}

#[test]
fn arguments_are_passed_through() {
    let sandbox = Sandbox::new();
    sandbox.taco(&["add", "greet", "--", "echo"]);

    assert_snapshot!("passthrough_arguments", sandbox.taco(&["greet", "world"]));

    // Flags need the `--` separator
    assert_snapshot!(
        "passthrough_flags",
        sandbox.taco(&["greet", "--", "--flag"])
    );
}

#[test]
fn exit_codes_are_mirrored() {
    let sandbox = Sandbox::new();
    sandbox.taco(&["add", "fail", "--", "exit", "7"]);

    assert_snapshot!("exit_code_mirrored", sandbox.taco(&["fail"]));
}

#[test]
fn the_print_flag_shows_the_command_instead_of_running_it() {
    let sandbox = Sandbox::new();
    sandbox.taco(&["add", "greet", "--", "echo", "hello"]);

    assert_snapshot!("print_flag", sandbox.taco(&["greet", "--print"]));
}

#[test]
fn commands_are_inherited_from_parent_directories() {
    let sandbox = Sandbox::new();
    sandbox.taco(&["add", "greet", "--", "echo", "from-parent"]);

    assert_snapshot!(
        "inherited_from_parent",
        sandbox.taco_in(&sandbox.nested(), &["greet"])
    );
}

#[test]
fn deeper_projects_win_over_parents() {
    let sandbox = Sandbox::new();
    sandbox.taco(&["add", "greet", "--", "echo", "from-parent"]);
    sandbox.taco_in(
        &sandbox.nested(),
        &["add", "greet", "--", "echo", "from-nested"],
    );

    assert_snapshot!(
        "deeper_project_wins",
        sandbox.taco_in(&sandbox.nested(), &["greet"])
    );

    // The parent is not affected
    assert_snapshot!("parent_unaffected", sandbox.taco(&["greet"]));
}

#[test]
fn aliased_projects_are_inherited() {
    let sandbox = Sandbox::new();
    sandbox.write_config(&format!(
        r#"{{"projects": {{"vitest": {{"tdd": "echo vitest-tdd"}}}}, "aliases": {{"{}": ["vitest"]}}}}"#,
        sandbox.project.display()
    ));

    assert_snapshot!(
        "aliased_project_inherited",
        sandbox.taco_in(&sandbox.nested(), &["tdd"])
    );
}

#[test]
fn user_commands_win_over_builtins() {
    let sandbox = Sandbox::new();

    // Shadowing a builtin prints a note
    assert_snapshot!(
        "shadowing_add_note",
        sandbox.taco(&["add", "print", "--", "echo", "MY-PRINT"])
    );

    // The user command wins
    assert_snapshot!("shadowed_user_command_wins", sandbox.taco(&["print"]));

    // The builtin stays reachable through the `taco` namespace
    assert_snapshot!("builtin_via_namespace", sandbox.taco(&["taco", "print"]));
}

#[test]
fn taco_is_a_reserved_command_name() {
    let sandbox = Sandbox::new();

    assert_snapshot!(
        "taco_reserved",
        sandbox.taco(&["add", "taco", "--", "echo", "nope"])
    );
}

#[test]
fn double_dash_always_runs_a_user_command() {
    let sandbox = Sandbox::new();
    sandbox.taco(&["add", "greet", "--", "echo"]);

    assert_snapshot!(
        "double_dash_escape",
        sandbox.taco(&["--", "greet", "hello"])
    );
}

#[test]
fn which_shows_where_a_command_is_defined() {
    let sandbox = Sandbox::new();
    sandbox.taco(&["add", "greet", "--", "echo", "from-parent"]);
    sandbox.taco_in(
        &sandbox.nested(),
        &["add", "greet", "--", "echo", "from-nested"],
    );

    assert_snapshot!(
        "which_with_shadowed_definitions",
        sandbox.taco_in(&sandbox.nested(), &["which", "greet"])
    );
}

#[test]
fn rm_points_at_the_winning_definition_for_inherited_commands() {
    let sandbox = Sandbox::new();
    sandbox.taco(&["add", "greet", "--", "echo", "hello"]);

    // Removing an inherited command tells you where it is defined instead
    assert_snapshot!(
        "rm_inherited_hint",
        sandbox.taco_in(&sandbox.nested(), &["rm", "greet"])
    );

    // Removing it where it is defined works, and cleans up the empty project entry
    assert_snapshot!("rm_local", sandbox.taco(&["rm", "greet"]));
    assert_snapshot!("rm_config_after", sandbox.config_contents());
}

#[test]
fn unalias_hints_at_the_parent_attachment() {
    let sandbox = Sandbox::new();
    sandbox.write_config(&format!(
        r#"{{"projects": {{"vitest": {{"tdd": "echo vitest-tdd"}}}}, "aliases": {{"{}": ["vitest"]}}}}"#,
        sandbox.project.display()
    ));

    assert_snapshot!(
        "unalias_parent_hint",
        sandbox.taco_in(&sandbox.nested(), &["unalias", "vitest"])
    );
    assert_snapshot!("unalias_local", sandbox.taco(&["unalias", "vitest"]));
    assert_snapshot!("unalias_config_after", sandbox.config_contents());
}

#[test]
fn doctor_finds_and_fixes_stale_projects() {
    let sandbox = Sandbox::new();
    sandbox.write_config(r#"{"projects": {"/taco-test-does-not-exist": {"build": "make"}}}"#);

    assert_snapshot!("doctor_report", sandbox.taco(&["doctor"]));

    // Declining leaves the config untouched
    assert_snapshot!(
        "doctor_fix_declined",
        sandbox.taco_stdin(&sandbox.project, &["doctor", "--fix"], "n\n")
    );
    assert_snapshot!("doctor_config_after_decline", sandbox.config_contents());

    // Accepting cleans it up
    assert_snapshot!(
        "doctor_fix_accepted",
        sandbox.taco_stdin(&sandbox.project, &["doctor", "--fix"], "y\n")
    );
    assert_snapshot!("doctor_config_after_fix", sandbox.config_contents());

    assert_snapshot!("doctor_healthy", sandbox.taco(&["doctor"]));
}

#[test]
fn typos_get_a_suggestion() {
    let sandbox = Sandbox::new();
    sandbox.taco(&["add", "test", "--", "echo", "testing"]);

    assert_snapshot!("typo_suggestion", sandbox.taco(&["tets"]));

    // Builtins are suggested too
    assert_snapshot!("typo_builtin_suggestion", sandbox.taco(&["doctro"]));

    // And inside the `taco` namespace
    assert_snapshot!(
        "typo_namespace_suggestion",
        sandbox.taco(&["taco", "doctro"])
    );
}

#[test]
fn the_config_location_can_be_overridden() {
    let sandbox = Sandbox::new();
    sandbox.taco(&["add", "greet", "--", "echo", "hello"]);

    assert_snapshot!("config_file_contents", sandbox.config_contents());
}

#[test]
fn local_taco_json_commands_are_available() {
    let sandbox = Sandbox::new();
    sandbox.write_local_config(r#"{"commands": {"build": "echo local-build"}}"#);

    assert_snapshot!("local_command_runs", sandbox.taco(&["build"]));
    assert_snapshot!(
        "local_command_inherited",
        sandbox.taco_in(&sandbox.nested(), &["build"])
    );
    assert_snapshot!("local_command_print", sandbox.taco(&["print"]));
}

#[test]
fn personal_commands_win_over_local_ones() {
    let sandbox = Sandbox::new();
    sandbox.write_local_config(r#"{"commands": {"build": "echo local-build"}}"#);
    sandbox.taco(&["add", "build", "--", "echo", "personal-build"]);

    assert_snapshot!("personal_wins_over_local", sandbox.taco(&["build"]));
    assert_snapshot!("which_local_shadowed", sandbox.taco(&["which", "build"]));
}

#[test]
fn rm_points_at_the_local_file() {
    let sandbox = Sandbox::new();
    sandbox.write_local_config(r#"{"commands": {"build": "echo local-build"}}"#);

    assert_snapshot!("rm_local_file_hint", sandbox.taco(&["rm", "build"]));
}

#[test]
fn a_broken_local_config_is_reported_but_does_not_block_builtins() {
    let sandbox = Sandbox::new();
    sandbox.write_local_config("{broken");

    // Running a command reports the broken file. Not snapshotted: the eyre report includes
    // source locations that would make the snapshot brittle.
    let output = sandbox.taco(&["build"]);
    assert!(output.contains("exit code: 1"), "{output}");
    assert!(
        output.contains("Invalid config file: <root>/project/.taco.json"),
        "{output}"
    );

    // Builtins that do not resolve commands keep working
    assert_snapshot!(
        "broken_local_config_add_still_works",
        sandbox.taco(&["add", "greet", "--", "echo", "hello"])
    );
}

#[test]
fn a_broken_config_edit_can_be_restored() {
    let sandbox = Sandbox::new();
    sandbox.taco(&["add", "greet", "--", "echo", "hello"]);
    let valid = sandbox.config_contents();

    let breaker = sandbox.editor("break-config", r#"echo '{broken' > "$1""#);

    assert_snapshot!(
        "config_restore",
        sandbox.taco_with_editor(&breaker, &["config"], "r\n")
    );
    assert_eq!(sandbox.config_contents(), valid);
}

#[test]
fn a_broken_config_edit_can_be_edited_again() {
    let sandbox = Sandbox::new();
    sandbox.taco(&["add", "greet", "--", "echo", "hello"]);
    let valid = sandbox.config_contents();

    // Breaks the config on the first run, undoes the damage on the second
    let marker = sandbox.base.join("second-run");
    let fixer = sandbox.editor(
        "fix-config-on-retry",
        &format!(
            r#"if [ -f "{marker}" ]; then cp "{marker}" "$1"; else cp "$1" "{marker}"; echo '{{broken' > "$1"; fi"#,
            marker = marker.display()
        ),
    );

    assert_snapshot!(
        "config_edit_again",
        sandbox.taco_with_editor(&fixer, &["config"], "e\n")
    );
    assert_eq!(sandbox.config_contents(), valid);
}

#[test]
fn a_broken_config_edit_can_be_kept() {
    let sandbox = Sandbox::new();
    sandbox.taco(&["add", "greet", "--", "echo", "hello"]);

    let breaker = sandbox.editor("break-config", r#"echo '{broken' > "$1""#);

    assert_snapshot!(
        "config_keep_broken",
        sandbox.taco_with_editor(&breaker, &["config"], "k\n")
    );
    assert_snapshot!("config_broken_contents", sandbox.config_contents());
}

#[test]
fn an_unanswered_recovery_prompt_keeps_the_broken_config() {
    let sandbox = Sandbox::new();
    sandbox.taco(&["add", "greet", "--", "echo", "hello"]);

    let breaker = sandbox.editor("break-config", r#"echo '{broken' > "$1""#);

    // EOF on stdin, like non-interactive use, reports the failure and bails
    assert_snapshot!(
        "config_prompt_eof",
        sandbox.taco_with_editor(&breaker, &["config"], "")
    );
    assert!(sandbox.config_contents().contains("{broken"));
}

#[test]
fn restore_is_not_offered_when_the_config_was_already_broken() {
    let sandbox = Sandbox::new();
    sandbox.write_config("{already broken");

    let breaker = sandbox.editor("break-config", r#"echo '{broken' > "$1""#);

    // There is no valid config to restore, so `r` is not an option and falls through to EOF
    assert_snapshot!(
        "config_no_restore_option",
        sandbox.taco_with_editor(&breaker, &["config"], "r\n")
    );
}

#[test]
fn add_local_stores_the_command_in_the_taco_json() {
    let sandbox = Sandbox::new();

    assert_snapshot!(
        "add_local",
        sandbox.taco(&["add", "--local", "build", "--", "echo", "local-build"])
    );
    assert_snapshot!("add_local_file", sandbox.local_config_contents());
    assert_snapshot!("add_local_runs", sandbox.taco(&["build"]));
}

#[test]
fn add_local_notes_when_a_personal_command_shadows_it() {
    let sandbox = Sandbox::new();
    sandbox.taco(&["add", "build", "--", "echo", "personal-build"]);

    assert_snapshot!(
        "add_local_shadowed_note",
        sandbox.taco(&["add", "--local", "build", "--", "echo", "local-build"])
    );
    assert_snapshot!("add_local_shadowed_runs", sandbox.taco(&["build"]));
}

#[test]
fn rm_local_removes_and_cleans_up_the_file() {
    let sandbox = Sandbox::new();
    sandbox.taco(&["add", "--local", "build", "--", "echo", "local-build"]);
    sandbox.taco(&["add", "--local", "test", "--", "echo", "local-test"]);

    assert_snapshot!("rm_local_removed", sandbox.taco(&["rm", "--local", "build"]));
    assert_snapshot!("rm_local_file_after", sandbox.local_config_contents());

    // Removing the last command deletes the now empty file
    assert_snapshot!("rm_local_last", sandbox.taco(&["rm", "--local", "test"]));
    assert!(!sandbox.project.join(".taco.json").exists());

    // Removing from a directory without a `.taco.json` suggests nothing to remove
    assert_snapshot!(
        "rm_local_missing",
        sandbox.taco(&["rm", "--local", "build"])
    );
}

#[test]
fn edit_local_edits_the_taco_json() {
    let sandbox = Sandbox::new();
    sandbox.taco(&["add", "--local", "build", "--", "echo", "local-build"]);

    let editor = sandbox.editor("edit-command", r#"echo 'echo edited-build' > "$1""#);
    assert_snapshot!(
        "edit_local",
        sandbox.taco_with_editor(&editor, &["edit", "--local", "build"], "")
    );
    assert_snapshot!("edit_local_runs", sandbox.taco(&["build"]));

    // Editing a command that is not in the file suggests the ones that are
    assert_snapshot!(
        "edit_local_typo",
        sandbox.taco_with_editor(&editor, &["edit", "--local", "biuld"], "")
    );
}

#[test]
fn config_local_opens_the_taco_json_with_the_recovery_prompt() {
    let sandbox = Sandbox::new();
    sandbox.taco(&["add", "--local", "build", "--", "echo", "local-build"]);
    let valid = sandbox.local_config_contents();

    let breaker = sandbox.editor("break-config", r#"echo '{broken' > "$1""#);
    assert_snapshot!(
        "config_local_restore",
        sandbox.taco_with_editor(&breaker, &["config", "--local"], "r\n")
    );
    assert_eq!(sandbox.local_config_contents(), valid);
}

#[test]
fn config_local_creates_the_file_when_missing() {
    let sandbox = Sandbox::new();

    let noop = sandbox.editor("noop-editor", ":");
    assert_snapshot!(
        "config_local_creates_file",
        sandbox.taco_with_editor(&noop, &["config", "--local"], "")
    );
    assert_snapshot!("config_local_default_file", sandbox.local_config_contents());
}

#[test]
fn a_flat_taco_json_is_rejected() {
    let sandbox = Sandbox::new();
    sandbox.write_local_config(r#"{"build": "echo local-build"}"#);

    // Not snapshotted: the eyre report includes source locations that would make it brittle
    let output = sandbox.taco(&["print"]);
    assert!(output.contains("exit code: 1"), "{output}");
    assert!(
        output.contains("Invalid config file: <root>/project/.taco.json"),
        "{output}"
    );
    assert!(output.contains("unknown field"), "{output}");
}
