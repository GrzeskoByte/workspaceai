use workspaceai_client::files::*;

#[test]
fn safe_paths_stay_bare_and_others_are_single_quoted() {
    assert_eq!(quote_path("/a/b-c_d.txt"), "/a/b-c_d.txt");
    assert_eq!(quote_path("/a/my file.txt"), "'/a/my file.txt'");
    assert_eq!(quote_path("/a/it's"), "'/a/it'\\''s'");
    assert_eq!(quote_path("/a/$(rm -rf ~)"), "'/a/$(rm -rf ~)'");
    assert_eq!(quote_path(""), "''");
}

#[test]
fn several_paths_end_with_a_space() {
    assert_eq!(quote_paths(&["/a".into(), "/b c".into()]), "/a '/b c' ");
    assert_eq!(quote_paths(&[]), "");
}

#[test]
fn paths_with_control_characters_are_never_pasted() {
    use workspaceai_client::files::is_pasteable;
    assert!(is_pasteable("/a/b c.txt"));
    assert!(!is_pasteable("/a/x\ny"));
    assert!(!is_pasteable("/a/\x1b[31m"));
    assert!(!is_pasteable("/a/\u{7f}"));
    assert_eq!(quote_paths(&["/ok".into(), "/bad\nrm -rf ~".into(), "/b c".into()]), "/ok '/b c' ");
}
