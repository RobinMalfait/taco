use std::io::Write;
use std::{env, fs, process};
use uuid::Uuid;

/// Open the user's editor ($VISUAL, falling back to $EDITOR) with the given contents pre-filled,
/// and return the edited result. Returns `None` when no editor is configured, or the editor
/// fails/aborts.
pub fn rich_edit(contents: &str) -> Option<String> {
    let editor = env::var("VISUAL").or_else(|_| env::var("EDITOR")).ok()?;

    // The editor may include arguments, e.g. `code --wait`
    let mut parts = editor.split_whitespace();
    let program = parts.next()?;

    let path = env::temp_dir().join(format!("taco-{}.sh", Uuid::new_v4()));

    // Create the file exclusively and only readable by the current user, since the command being
    // edited could contain sensitive information.
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }

    let result = options
        .open(&path)
        .and_then(|mut file| file.write_all(contents.as_bytes()))
        .ok()
        .and_then(|()| {
            process::Command::new(program)
                .args(parts)
                .arg(&path)
                .status()
                .ok()
        })
        .filter(std::process::ExitStatus::success)
        .and_then(|_| fs::read_to_string(&path).ok());

    // Best-effort cleanup; the editor may already have removed the file.
    let _ = fs::remove_file(&path);

    result
}
