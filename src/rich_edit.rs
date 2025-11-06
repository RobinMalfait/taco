use std::{env, fs, process};
use uuid::Uuid;

pub fn rich_edit(contents: Option<&str>) -> Option<String> {
    let Ok(editor) = env::var("EDITOR") else {
        return None;
    };

    let mut dir = env::temp_dir();
    dir.push(&format!("{}.sh", Uuid::new_v4()));
    let file_path = dir.to_str().unwrap();

    fs::write(file_path, contents.unwrap_or("")).unwrap();

    let result = match process::Command::new(editor).arg(file_path).status() {
        Ok(status) => {
            if status.success() {
                fs::read_to_string(file_path).ok()
            } else {
                None
            }
        }
        Err(_) => None,
    };

    // Cleanup
    fs::remove_file(file_path).unwrap();

    result
}
