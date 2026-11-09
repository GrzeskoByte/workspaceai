//! Files dropped on, or attached to, a terminal session: the path is pasted shell-quoted.

/// The path as one shell word: bare when it only has safe characters, else single-quoted.
pub fn quote_path(path: &str) -> String {
    if !path.is_empty() && path.chars().all(|c| c.is_ascii_alphanumeric() || "/._-+=:,@%".contains(c)) {
        return path.to_string();
    }
    format!("'{}'", path.replace('\'', "'\\''"))
}

/// Several paths, space separated, with a trailing space so the user can keep typing.
pub fn quote_paths(paths: &[String]) -> String {
    paths.iter().map(|p| format!("{} ", quote_path(p))).collect()
}
