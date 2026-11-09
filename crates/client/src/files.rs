//! Files dropped on, or attached to, a terminal session: the path is pasted shell-quoted.

/// The path as one shell word: bare when it only has safe characters, else single-quoted.
pub fn quote_path(path: &str) -> String {
    if !path.is_empty() && path.chars().all(|c| c.is_ascii_alphanumeric() || "/._-+=:,@%".contains(c)) {
        return path.to_string();
    }
    format!("'{}'", path.replace('\'', "'\\''"))
}

/// A path with a control character (newline, ESC, DEL, ...) cannot be pasted safely: the terminal would act on it
/// whatever the shell quoting says. Such paths are left out.
pub fn is_pasteable(path: &str) -> bool {
    !path.chars().any(char::is_control)
}

/// Several paths, space separated, with a trailing space so the user can keep typing. Paths that are not
/// `is_pasteable` are skipped.
pub fn quote_paths(paths: &[String]) -> String {
    paths.iter().filter(|p| is_pasteable(p)).map(|p| format!("{} ", quote_path(p))).collect()
}
