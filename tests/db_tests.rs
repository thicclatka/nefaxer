//! DB tests: path_count_from_db, load_index round-trip, and file-DB fixture.

use nefaxer::engine::{load_index, open_db, open_db_in_memory, path_count_from_db};
use std::path::PathBuf;

const INSERT_PATH_SQL: &str =
    "INSERT OR REPLACE INTO paths (path, mtime_ns, size, hash) VALUES (?1, ?2, ?3, ?4)";

#[test]
fn test_path_count_from_db_empty() {
    let conn = open_db_in_memory().unwrap();
    assert_eq!(path_count_from_db(&conn), Some(0));
}

#[test]
fn test_path_count_from_db_after_insert() {
    let conn = open_db_in_memory().unwrap();
    conn.execute(
        INSERT_PATH_SQL,
        rusqlite::params!["a/b", 100_i64, 10_i64, None::<Vec<u8>>],
    )
    .unwrap();
    conn.execute(
        INSERT_PATH_SQL,
        rusqlite::params!["c/d", 200_i64, 20_i64, None::<Vec<u8>>],
    )
    .unwrap();
    assert_eq!(path_count_from_db(&conn), Some(2));

    conn.execute(
        INSERT_PATH_SQL,
        rusqlite::params!["e/f", 300_i64, 30_i64, Some(vec![0u8; 32])],
    )
    .unwrap();
    assert_eq!(path_count_from_db(&conn), Some(3));
}

#[test]
fn test_load_index_round_trip() {
    let conn = open_db_in_memory().unwrap();
    let rows = [
        ("rel/path/a", 1000_i64, 100_i64, None::<Vec<u8>>),
        ("rel/path/b", 2000_i64, 200_i64, Some(vec![1u8; 32])),
        ("single", 0_i64, 0_i64, None::<Vec<u8>>),
    ];
    for (path, mtime_ns, size, hash) in &rows {
        conn.execute(
            INSERT_PATH_SQL,
            rusqlite::params![path, mtime_ns, size, hash],
        )
        .unwrap();
    }

    let map = load_index(&conn).unwrap();
    assert_eq!(map.len(), 3);

    assert_eq!(
        map.get(&PathBuf::from("rel/path/a")),
        Some(&(1000, 100_u64, None))
    );
    assert_eq!(
        map.get(&PathBuf::from("rel/path/b")),
        Some(&(2000, 200_u64, Some(vec![1u8; 32])))
    );
    assert_eq!(
        map.get(&PathBuf::from("single")),
        Some(&(0_i64, 0_u64, None))
    );
}

/// Uses tests/fixtures/.nefaxer_simple: create if missing (empty schema), then path_count → 0.
#[test]
fn test_path_count_from_db_file_fixture_simple() {
    let fixtures_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures");
    let db_path = fixtures_dir.join(".nefaxer_simple");
    std::fs::create_dir_all(&fixtures_dir).unwrap();
    let conn = open_db(&db_path, None).unwrap();
    assert_eq!(path_count_from_db(&conn), Some(0));
}

/// Uses tests/fixtures/.nefaxer_complex: real index of this repo (diskinfo wiped).
/// Guards path_count_from_db and load_index on a real-sized fixture.
#[test]
fn test_path_count_and_load_index_complex_fixture() {
    let db_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join(".nefaxer_complex");
    if !db_path.exists() {
        eprintln!(
            "skip: {} not found (copy a .nefaxer here and rename)",
            db_path.display()
        );
        return;
    }
    let conn = open_db(&db_path, None).unwrap();
    let count = path_count_from_db(&conn).expect("COUNT(*) should succeed");
    assert!(count > 0, "complex fixture should have at least one path");

    let map = load_index(&conn).unwrap();
    assert_eq!(map.len(), count, "load_index len should match path count");

    // Sanity: repo fixture should contain these paths
    let expected = ["Cargo.toml", "src/main.rs", "src/lib.rs"];
    for p in &expected {
        assert!(
            map.contains_key(&PathBuf::from(p)),
            "complex fixture should contain {}",
            p
        );
    }
}
