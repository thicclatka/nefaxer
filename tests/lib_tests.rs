use nefaxer::Entry;
use nefaxer::engine::{
    StoredMeta, entry_needs_update, glob_match, hash_equals, mtime_changed, path_relative_to,
    path_to_db_string, should_include_in_walk,
};
use std::collections::HashMap;
use std::path::PathBuf;

// --- path_relative_to ---

#[test]
fn test_path_relative_under_base() {
    let base = PathBuf::from("/foo/bar");
    let path = PathBuf::from("/foo/bar/baz/qux");
    assert_eq!(
        path_relative_to(&path, &base),
        Some(PathBuf::from("baz/qux"))
    );
}

#[test]
fn test_path_relative_not_under_base() {
    let base = PathBuf::from("/foo/bar");
    let path = PathBuf::from("/other/qux");
    assert_eq!(path_relative_to(&path, &base), None);
}

#[test]
fn test_path_relative_path_equals_base() {
    let base = PathBuf::from("/foo/bar");
    let path = PathBuf::from("/foo/bar");
    assert_eq!(path_relative_to(&path, &base), Some(PathBuf::new()));
}

#[test]
fn test_path_relative_with_dotdot() {
    let base = PathBuf::from("/foo/bar");
    let path = PathBuf::from("/foo/bar/../bar/baz");
    assert_eq!(
        path_relative_to(&path, &base),
        Some(PathBuf::from("../bar/baz"))
    );
}

// --- path_to_db_string (path normalization for DB portability) ---

#[test]
fn test_path_to_db_string_forward_slashes() {
    assert_eq!(
        path_to_db_string(&PathBuf::from("src/main.rs")),
        "src/main.rs"
    );
}

#[test]
fn test_path_to_db_string_normalizes_backslashes() {
    assert_eq!(
        path_to_db_string(&PathBuf::from("src\\main.rs")),
        "src/main.rs"
    );
}

// --- mtime_changed ---

#[test]
fn test_mtime_changed_same_mtime() {
    let t = 1_000_000_000i64;
    assert!(!mtime_changed(t, t, 0));
    assert!(!mtime_changed(t, t, 100));
}

#[test]
fn test_mtime_changed_within_window() {
    let old = 1_000_000_000i64;
    let window = 50i64;
    assert!(!mtime_changed(old + 0, old, window));
    assert!(!mtime_changed(old + 50, old, window));
    assert!(!mtime_changed(old - 50, old, window));
}

#[test]
fn test_mtime_changed_outside_window() {
    let old = 1_000_000_000i64;
    let window = 50i64;
    assert!(mtime_changed(old + 51, old, window));
    assert!(mtime_changed(old - 51, old, window));
}

#[test]
fn test_mtime_changed_zero_tolerance() {
    let t = 1_000_000_000i64;
    assert!(!mtime_changed(t, t, 0));
    assert!(mtime_changed(t + 1, t, 0));
    assert!(mtime_changed(t - 1, t, 0));
}

// --- hash_equals ---

#[test]
fn test_hash_equals_none_none() {
    assert!(hash_equals(&None, &None));
}

#[test]
fn test_hash_equals_some_some_same() {
    let a = [1u8; 32];
    let b = vec![1u8; 32];
    assert!(hash_equals(&Some(a), &Some(b)));
}

#[test]
fn test_hash_equals_some_some_different() {
    let a = [1u8; 32];
    let mut b = vec![1u8; 32];
    b[0] = 2;
    assert!(!hash_equals(&Some(a), &Some(b)));
}

#[test]
fn test_hash_equals_none_some() {
    assert!(!hash_equals(&None, &Some(vec![0u8; 32])));
}

#[test]
fn test_hash_equals_some_none() {
    assert!(!hash_equals(&Some([0u8; 32]), &None));
}

// --- glob_match / should_include_in_walk ---

#[test]
fn test_glob_match_literal() {
    assert!(glob_match("node_modules", "node_modules"));
    assert!(!glob_match("node_modules", "node_module"));
}

#[test]
fn test_glob_match_star() {
    assert!(glob_match("*.log", "foo.log"));
    assert!(glob_match("*.log", ".log"));
    assert!(!glob_match("*.log", "foo.log.txt"));
    assert!(glob_match("node_*", "node_modules"));
}

#[test]
fn test_glob_match_negation_stripped() {
    assert!(glob_match("!node_modules", "node_modules"));
}

#[test]
fn test_should_include_root_excluded() {
    let root = PathBuf::from("/foo");
    assert!(!should_include_in_walk(&root, &root, &None, &None, &[]));
}

#[test]
fn test_should_include_db_canonical_skipped() {
    let root = PathBuf::from("/foo");
    let db = PathBuf::from("/foo/.nefaxer");
    assert!(!should_include_in_walk(
        &db,
        &root,
        &Some(db.clone()),
        &None,
        &[]
    ));
}

#[test]
fn test_should_include_temp_canonical_skipped() {
    let root = PathBuf::from("/foo");
    let temp = PathBuf::from("/foo/.nefaxer.tmp");
    assert!(!should_include_in_walk(
        &temp,
        &root,
        &None,
        &Some(temp.clone()),
        &[]
    ));
}

#[test]
fn test_should_include_exclude_pattern_name() {
    let root = PathBuf::from("/foo");
    let path = PathBuf::from("/foo/node_modules");
    assert!(!should_include_in_walk(
        &path,
        &root,
        &None,
        &None,
        &["node_modules".to_string()]
    ));
}

#[test]
fn test_should_include_exclude_pattern_glob() {
    let root = PathBuf::from("/foo");
    let path = PathBuf::from("/foo/bar/baz.log");
    assert!(!should_include_in_walk(
        &path,
        &root,
        &None,
        &None,
        &["*.log".to_string()]
    ));
}

#[test]
fn test_should_include_not_excluded() {
    let root = PathBuf::from("/foo");
    let path = PathBuf::from("/foo/bar/baz.txt");
    assert!(should_include_in_walk(
        &path,
        &root,
        &None,
        &None,
        &["*.log".to_string(), "node_modules".to_string()]
    ));
}

// --- entry_needs_update ---

fn entry(path: &str, mtime_ns: i64, size: u64, hash: Option<[u8; 32]>) -> Entry {
    Entry {
        path: PathBuf::from(path),
        mtime_ns,
        size,
        hash,
    }
}

fn meta(mtime_ns: i64, size: u64, hash: Option<Vec<u8>>) -> StoredMeta {
    (mtime_ns, size, hash)
}

#[test]
fn test_entry_needs_update_new_path() {
    let existing: HashMap<PathBuf, StoredMeta> = HashMap::new();
    assert!(entry_needs_update(
        &entry("a/b", 100, 10, None),
        &existing,
        0
    ));
}

#[test]
fn test_entry_needs_update_same_mtime_size_hash() {
    let mut existing = HashMap::new();
    existing.insert(PathBuf::from("a/b"), meta(100, 10, Some(vec![1; 32])));
    assert!(!entry_needs_update(
        &entry("a/b", 100, 10, Some([1; 32])),
        &existing,
        0
    ));
}

#[test]
fn test_entry_needs_update_different_mtime() {
    let mut existing = HashMap::new();
    existing.insert(PathBuf::from("a/b"), meta(100, 10, Some(vec![1; 32])));
    assert!(entry_needs_update(
        &entry("a/b", 101, 10, Some([1; 32])),
        &existing,
        0
    ));
}

#[test]
fn test_entry_needs_update_different_size() {
    let mut existing = HashMap::new();
    existing.insert(PathBuf::from("a/b"), meta(100, 10, None));
    assert!(entry_needs_update(
        &entry("a/b", 100, 11, None),
        &existing,
        0
    ));
}

#[test]
fn test_entry_needs_update_different_hash() {
    let mut existing = HashMap::new();
    existing.insert(PathBuf::from("a/b"), meta(100, 10, Some(vec![1; 32])));
    let mut h = [1u8; 32];
    h[0] = 2;
    assert!(entry_needs_update(
        &entry("a/b", 100, 10, Some(h)),
        &existing,
        0
    ));
}

#[test]
fn test_entry_needs_update_within_tolerance() {
    let mut existing = HashMap::new();
    existing.insert(PathBuf::from("a/b"), meta(100, 10, Some(vec![1; 32])));
    assert!(!entry_needs_update(
        &entry("a/b", 150, 10, Some([1; 32])),
        &existing,
        50
    ));
}

#[test]
fn test_entry_needs_update_outside_tolerance() {
    let mut existing = HashMap::new();
    existing.insert(PathBuf::from("a/b"), meta(100, 10, Some(vec![1; 32])));
    assert!(entry_needs_update(
        &entry("a/b", 151, 10, Some([1; 32])),
        &existing,
        50
    ));
}
