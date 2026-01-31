use nefaxer::engine::path_relative_to;
use std::path::PathBuf;

#[test]
fn test_path_relative() {
    let base = PathBuf::from("/foo/bar");
    let path = PathBuf::from("/foo/bar/baz/qux");
    assert_eq!(
        path_relative_to(&path, &base),
        Some(PathBuf::from("baz/qux"))
    );
}
