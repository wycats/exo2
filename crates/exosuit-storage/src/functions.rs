//! SQL scalar functions for reactive tracing.
//!
//! This module provides SQL functions that can be used in queries to compute
//! content hashes and validate traces.

use rusqlite::functions::FunctionFlags;
use rusqlite::types::ValueRef;
use rusqlite::{Connection, Result};

use crate::DatabaseError;

/// Register all reactive SQL functions with a connection.
///
/// This registers:
/// - `content_hash(...)`: Compute BLAKE3 hash of arguments
///
/// # Example
///
/// ```ignore
/// register_functions(&conn)?;
/// let hash: Vec<u8> = conn.query_row(
///     "SELECT content_hash('hello', 42)",
///     [],
///     |row| row.get(0)
/// )?;
/// ```
pub fn register_functions(conn: &Connection) -> Result<(), DatabaseError> {
    register_content_hash(conn)?;
    Ok(())
}

/// Register the content_hash() scalar function.
///
/// `content_hash(arg1, arg2, ...)` computes a BLAKE3 hash of all arguments.
/// This is used to compute row digests for content-based invalidation.
fn register_content_hash(conn: &Connection) -> Result<(), DatabaseError> {
    conn.create_scalar_function(
        "content_hash",
        -1, // Variable number of arguments
        FunctionFlags::SQLITE_UTF8 | FunctionFlags::SQLITE_DETERMINISTIC,
        |ctx| {
            use blake3::Hasher;

            let mut hasher = Hasher::new();

            for i in 0..ctx.len() {
                let value = ctx.get_raw(i);
                match value {
                    ValueRef::Null => {
                        hasher.update(b"\x00");
                    }
                    ValueRef::Integer(n) => {
                        hasher.update(b"\x01");
                        hasher.update(&n.to_le_bytes());
                    }
                    ValueRef::Real(f) => {
                        hasher.update(b"\x02");
                        hasher.update(&f.to_le_bytes());
                    }
                    ValueRef::Text(s) => {
                        hasher.update(b"\x03");
                        hasher.update(s);
                    }
                    ValueRef::Blob(b) => {
                        hasher.update(b"\x04");
                        hasher.update(b);
                    }
                }
            }

            Ok(hasher.finalize().as_bytes().to_vec())
        },
    )?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::open_memory_database;

    #[test]
    fn test_content_hash_basic() {
        let db = open_memory_database().expect("should create database");
        let conn = db.connection();
        register_functions(conn).expect("should register functions");

        // Hash a simple string
        let hash: Vec<u8> = conn
            .query_row("SELECT content_hash('hello')", [], |row| row.get(0))
            .expect("should compute hash");

        assert_eq!(hash.len(), 32);
    }

    #[test]
    fn test_content_hash_deterministic() {
        let db = open_memory_database().expect("should create database");
        let conn = db.connection();
        register_functions(conn).expect("should register functions");

        // Same inputs should produce same hash
        let hash1: Vec<u8> = conn
            .query_row("SELECT content_hash('hello', 42)", [], |row| row.get(0))
            .expect("should compute hash");
        let hash2: Vec<u8> = conn
            .query_row("SELECT content_hash('hello', 42)", [], |row| row.get(0))
            .expect("should compute hash");

        assert_eq!(hash1, hash2);
    }

    #[test]
    fn test_content_hash_different_inputs() {
        let db = open_memory_database().expect("should create database");
        let conn = db.connection();
        register_functions(conn).expect("should register functions");

        let hash1: Vec<u8> = conn
            .query_row("SELECT content_hash('hello')", [], |row| row.get(0))
            .expect("should compute hash");
        let hash2: Vec<u8> = conn
            .query_row("SELECT content_hash('world')", [], |row| row.get(0))
            .expect("should compute hash");

        assert_ne!(hash1, hash2);
    }

    #[test]
    fn test_content_hash_all_types() {
        let db = open_memory_database().expect("should create database");
        let conn = db.connection();
        register_functions(conn).expect("should register functions");

        // Test with all SQLite types
        let hash: Vec<u8> = conn
            .query_row(
                "SELECT content_hash(NULL, 42, 3.14, 'text', X'DEADBEEF')",
                [],
                |row| row.get(0),
            )
            .expect("should compute hash");

        assert_eq!(hash.len(), 32);
    }

    #[test]
    fn test_content_hash_empty() {
        let db = open_memory_database().expect("should create database");
        let conn = db.connection();
        register_functions(conn).expect("should register functions");

        // Hash with no arguments
        let hash: Vec<u8> = conn
            .query_row("SELECT content_hash()", [], |row| row.get(0))
            .expect("should compute hash");

        assert_eq!(hash.len(), 32);
    }
}
