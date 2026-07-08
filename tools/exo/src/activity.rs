//! Projection queries over the `agent_events` table.
//!
//! These functions provide read-only views into recent agent activity,
//! used by steering and context commands to surface what the agent has
//! been working on.

use chrono::{DateTime, Duration, Utc};
use exosuit_storage::OptionalExtension;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

use crate::event_db::{event_db_path, with_event_db};

/// The most frequently referenced entity in the recent time window.
#[derive(Debug, Clone, Serialize)]
pub struct ActiveEntity {
    pub entity_type: String,
    pub entity_id: String,
    pub event_count: usize,
}

/// The current working session, bounded by a 30-minute inactivity gap.
#[derive(Debug, Clone, Serialize)]
pub struct SessionWindow {
    pub session_start: String,
    pub duration_minutes: i64,
    pub event_count: i64,
}

/// A file path from recent `file_save` events with its save count.
#[derive(Debug, Clone, Serialize)]
pub struct RecentFileArea {
    pub file_path: String,
    pub save_count: usize,
}

/// Returns the most frequently referenced (`entity_type`, `entity_id`) pair
/// from the last 10 minutes, or `None` if there are no matching events.
pub fn active_entity(root: &Path) -> Option<ActiveEntity> {
    active_entity_from_db(&event_db_path(root))
}

pub fn active_entity_from_db(db_path: &Path) -> Option<ActiveEntity> {
    let cutoff = (Utc::now() - Duration::minutes(10)).to_rfc3339();
    with_event_db(db_path, |conn| {
        conn.query_row(
            "SELECT entity_type, entity_id, COUNT(*) as cnt
             FROM agent_events
             WHERE timestamp > ?1
               AND entity_type IS NOT NULL AND entity_id IS NOT NULL
             GROUP BY entity_type, entity_id
             ORDER BY cnt DESC LIMIT 1",
            [&cutoff],
            |row| {
                Ok(ActiveEntity {
                    entity_type: row.get(0)?,
                    entity_id: row.get(1)?,
                    event_count: row.get::<_, i64>(2)? as usize,
                })
            },
        )
        .optional()
    })
    .flatten()
}

/// Returns the current session window, defined as all events after the
/// most recent 30-minute inactivity gap (looking back up to 24 hours).
pub fn session_window(root: &Path) -> Option<SessionWindow> {
    session_window_from_db(&event_db_path(root))
}

pub fn session_window_from_db(db_path: &Path) -> Option<SessionWindow> {
    let cutoff = (Utc::now() - Duration::hours(24)).to_rfc3339();
    let raw_timestamps: Vec<String> = with_event_db(db_path, |conn| {
        let mut stmt = conn.prepare(
            "SELECT timestamp FROM agent_events
             WHERE timestamp > ?1
             ORDER BY timestamp ASC",
        )?;
        let rows = stmt
            .query_map([&cutoff], |row| row.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    })?;

    let timestamps: Vec<DateTime<Utc>> = raw_timestamps
        .into_iter()
        .filter_map(|ts| {
            DateTime::parse_from_rfc3339(&ts)
                .ok()
                .map(|dt| dt.with_timezone(&Utc))
        })
        .collect();

    if timestamps.is_empty() {
        return None;
    }

    // Walk forward, resetting session_start at any gap > 30 min.
    let mut session_start_idx = 0;
    for i in 1..timestamps.len() {
        if timestamps[i] - timestamps[i - 1] > Duration::minutes(30) {
            session_start_idx = i;
        }
    }

    let session_start = timestamps[session_start_idx];
    let last = *timestamps.last()?;
    let duration = (last - session_start).num_minutes();
    let count = i64::try_from(timestamps.len() - session_start_idx).ok()?;

    Some(SessionWindow {
        session_start: session_start.to_rfc3339(),
        duration_minutes: duration,
        event_count: count,
    })
}

/// Summary of the session before the current one (across a >30min gap).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PreviousSessionSummary {
    pub event_count: i64,
    pub duration_minutes: i64,
    /// Most frequently referenced (`entity_type`, `entity_id`) in the previous session.
    pub primary_entity: Option<(String, String)>,
    /// Summary field from the last event of the previous session.
    pub last_action: String,
}

/// Compute a summary of the session before the most recent >30min gap.
///
/// Looks back up to 24 hours. Returns `None` if there is no gap (single
/// continuous session) or if there are too few events to form a previous session.
pub fn previous_session_summary(root: &Path) -> Option<PreviousSessionSummary> {
    previous_session_summary_from_db(&event_db_path(root))
}

pub fn previous_session_summary_from_db(db_path: &Path) -> Option<PreviousSessionSummary> {
    let cutoff = (Utc::now() - Duration::hours(24)).to_rfc3339();

    struct EventRow {
        timestamp: DateTime<Utc>,
        entity_type: Option<String>,
        entity_id: Option<String>,
        summary: String,
    }

    let raw_events: Vec<(String, Option<String>, Option<String>, String)> =
        with_event_db(db_path, |conn| {
            let mut stmt = conn.prepare(
                "SELECT timestamp, entity_type, entity_id, summary FROM agent_events
                 WHERE timestamp > ?1
                 ORDER BY timestamp ASC",
            )?;
            let rows = stmt
                .query_map([&cutoff], |row| {
                    Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
                })?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(rows)
        })?;

    let events: Vec<EventRow> = raw_events
        .into_iter()
        .filter_map(|(ts, entity_type, entity_id, summary)| {
            let timestamp = DateTime::parse_from_rfc3339(&ts)
                .ok()
                .map(|dt| dt.with_timezone(&Utc))?;
            Some(EventRow {
                timestamp,
                entity_type,
                entity_id,
                summary,
            })
        })
        .collect();

    if events.len() < 2 {
        return None;
    }

    // Find the last gap > 30 minutes (same logic as session_window).
    let mut last_gap_idx = None;
    for i in 1..events.len() {
        if events[i].timestamp - events[i - 1].timestamp > Duration::minutes(30) {
            last_gap_idx = Some(i);
        }
    }

    // No gap → no previous session.
    let gap_idx = last_gap_idx?;

    let prev_events = &events[..gap_idx];
    if prev_events.is_empty() {
        return None;
    }

    let event_count = i64::try_from(prev_events.len()).ok()?;
    let last_event = prev_events.last()?;
    let duration_minutes = (last_event.timestamp - prev_events[0].timestamp).num_minutes();

    // Mode of (entity_type, entity_id).
    let mut entity_counts: HashMap<(String, String), usize> = HashMap::new();
    for event in prev_events {
        if let (Some(et), Some(ei)) = (&event.entity_type, &event.entity_id) {
            *entity_counts.entry((et.clone(), ei.clone())).or_insert(0) += 1;
        }
    }
    let primary_entity = entity_counts
        .into_iter()
        .max_by_key(|(_, count)| *count)
        .map(|((et, ei), _)| (et, ei));

    let last_action = last_event.summary.clone();

    Some(PreviousSessionSummary {
        event_count,
        duration_minutes,
        primary_entity,
        last_action,
    })
}

/// Returns file paths from `file_save` events in the last 15 minutes,
/// grouped by path with save counts, most frequent first (up to 10).
pub fn recent_file_areas(root: &Path) -> Vec<RecentFileArea> {
    recent_file_areas_from_db(&event_db_path(root))
}

pub fn recent_file_areas_from_db(db_path: &Path) -> Vec<RecentFileArea> {
    let cutoff = (Utc::now() - Duration::minutes(15)).to_rfc3339();
    with_event_db(db_path, |conn| {
        let mut stmt = conn.prepare(
            "SELECT summary, COUNT(*) as cnt
             FROM agent_events
             WHERE event_type = 'file_save' AND timestamp > ?1
             GROUP BY summary
             ORDER BY cnt DESC LIMIT 10",
        )?;
        let rows = stmt
            .query_map([&cutoff], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    })
    .map(|rows| {
        rows.into_iter()
            .filter_map(|(summary, count)| {
                let path = summary.strip_prefix("file saved: ")?;
                Some(RecentFileArea {
                    file_path: path.to_string(),
                    save_count: count as usize,
                })
            })
            .collect()
    })
    .unwrap_or_default()
}

/// Extract the first path component (top-level directory) from a file path.
/// e.g. `src/auth/mod.rs` → `src/`, `Cargo.toml` → (empty — root-level file, skipped).
fn top_directory(path: &str) -> Option<&str> {
    let path = path.trim_start_matches('/');
    path.find('/').map(|idx| &path[..idx])
}

/// Infer the established scope of the current session by finding the most common
/// top-level directory prefixes from all `file_save` events in the session.
///
/// Returns the top 3 directory prefixes by save count. If no session data or
/// no file saves exist, returns an empty vec.
pub fn infer_entity_scope(root: &Path) -> Vec<String> {
    infer_entity_scope_from_db(&event_db_path(root))
}

pub fn infer_entity_scope_from_db(db_path: &Path) -> Vec<String> {
    let session = match session_window_from_db(db_path) {
        Some(s) => s,
        None => return vec![],
    };

    let summaries: Vec<String> = with_event_db(db_path, |conn| {
        let mut stmt = conn.prepare(
            "SELECT summary
             FROM agent_events
             WHERE event_type = 'file_save' AND timestamp > ?1",
        )?;
        let rows = stmt
            .query_map([&session.session_start], |row| row.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    })
    .unwrap_or_default();

    let mut dir_counts: HashMap<String, usize> = HashMap::new();
    for summary in summaries {
        if let Some(path) = summary.strip_prefix("file saved: ")
            && let Some(dir) = top_directory(path)
        {
            *dir_counts.entry(dir.to_string()).or_insert(0) += 1;
        }
    }

    let mut dirs: Vec<(String, usize)> = dir_counts.into_iter().collect();
    dirs.sort_by_key(|dir| std::cmp::Reverse(dir.1));
    dirs.into_iter().take(3).map(|(d, _)| d).collect()
}

/// Result of drift detection: files being edited outside the session's established scope.
#[derive(Debug, Clone)]
pub struct DriftDetection {
    /// Directory prefixes in recent file saves that are outside established scope.
    pub outside_dirs: Vec<String>,
}

/// Compare recent file areas against the session's established scope.
///
/// Returns `Some(DriftDetection)` if >50% of recent file saves are in directories
/// outside the established scope. Returns `None` if no drift detected or insufficient data.
pub fn detect_drift(recent_files: &[RecentFileArea], scope: &[String]) -> Option<DriftDetection> {
    if scope.is_empty() || recent_files.is_empty() {
        return None;
    }

    let mut inside_count: usize = 0;
    let mut outside_count: usize = 0;
    let mut outside_dirs: Vec<String> = Vec::new();

    for file in recent_files {
        let dir = top_directory(&file.file_path);
        match dir {
            Some(d) if scope.iter().any(|s| s == d) => {
                inside_count += file.save_count;
            }
            Some(d) => {
                outside_count += file.save_count;
                let d_str = d.to_string();
                if !outside_dirs.contains(&d_str) {
                    outside_dirs.push(d_str);
                }
            }
            None => {
                // Root-level files — don't count as drift
                inside_count += file.save_count;
            }
        }
    }

    let total = inside_count + outside_count;
    if total == 0 || outside_count * 2 <= total {
        return None;
    }

    Some(DriftDetection { outside_dirs })
}

/// Aggregated activity context from all three projection queries.
/// Used by `derive_entity_steering` to enrich steering with session awareness.
#[derive(Debug, Clone, Serialize)]
pub struct ActivityContext {
    pub active_entity: Option<ActiveEntity>,
    pub session: Option<SessionWindow>,
    pub recent_files: Vec<RecentFileArea>,
}

impl ActivityContext {
    pub fn collect(root: &Path) -> Self {
        Self::collect_from_db(&event_db_path(root))
    }

    pub fn collect_from_db(db_path: &Path) -> Self {
        Self {
            active_entity: active_entity_from_db(db_path),
            session: session_window_from_db(db_path),
            recent_files: recent_file_areas_from_db(db_path),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::SQLITE_DB_PATH;
    use chrono::Duration;
    use exosuit_storage::{open_database, open_memory_database, params};

    /// Insert a test event into an in-memory database.
    fn insert_event(
        conn: &exosuit_storage::Connection,
        text_id: &str,
        timestamp: &str,
        event_type: &str,
        entity_type: Option<&str>,
        entity_id: Option<&str>,
        summary: &str,
    ) {
        conn.execute(
            "INSERT INTO agent_events (text_id, timestamp, event_type, entity_type, entity_id, summary)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![text_id, timestamp, event_type, entity_type, entity_id, summary],
        )
        .unwrap();
    }

    // --- active_entity tests ---

    #[test]
    fn active_entity_returns_most_frequent() {
        let db = open_memory_database().unwrap();
        let conn = db.connection();
        let now = Utc::now();
        let recent = (now - Duration::minutes(5)).to_rfc3339();

        // 3 events for task/T1, 1 event for goal/G1
        insert_event(
            conn,
            "e1",
            &recent,
            "command",
            Some("task"),
            Some("T1"),
            "task start",
        );
        insert_event(
            conn,
            "e2",
            &recent,
            "command",
            Some("task"),
            Some("T1"),
            "task list",
        );
        insert_event(
            conn,
            "e3",
            &recent,
            "command",
            Some("task"),
            Some("T1"),
            "task complete",
        );
        insert_event(
            conn,
            "e4",
            &recent,
            "command",
            Some("goal"),
            Some("G1"),
            "goal list",
        );

        // Query directly against the in-memory DB instead of going through the file path.
        let cutoff = (Utc::now() - Duration::minutes(10)).to_rfc3339();
        let result: Option<ActiveEntity> = conn
            .query_row(
                "SELECT entity_type, entity_id, COUNT(*) as cnt
                 FROM agent_events
                 WHERE timestamp > ?1
                   AND entity_type IS NOT NULL AND entity_id IS NOT NULL
                 GROUP BY entity_type, entity_id
                 ORDER BY cnt DESC LIMIT 1",
                [&cutoff],
                |row| {
                    Ok(ActiveEntity {
                        entity_type: row.get(0)?,
                        entity_id: row.get(1)?,
                        event_count: row.get::<_, i64>(2)? as usize,
                    })
                },
            )
            .optional()
            .unwrap();

        let entity = result.unwrap();
        assert_eq!(entity.entity_type, "task");
        assert_eq!(entity.entity_id, "T1");
        assert_eq!(entity.event_count, 3);
    }

    #[test]
    fn active_entity_returns_none_for_old_events() {
        let db = open_memory_database().unwrap();
        let conn = db.connection();
        let old = (Utc::now() - Duration::minutes(20)).to_rfc3339();

        insert_event(
            conn,
            "e1",
            &old,
            "command",
            Some("task"),
            Some("T1"),
            "task start",
        );

        let cutoff = (Utc::now() - Duration::minutes(10)).to_rfc3339();
        let result: Option<ActiveEntity> = conn
            .query_row(
                "SELECT entity_type, entity_id, COUNT(*) as cnt
                 FROM agent_events
                 WHERE timestamp > ?1
                   AND entity_type IS NOT NULL AND entity_id IS NOT NULL
                 GROUP BY entity_type, entity_id
                 ORDER BY cnt DESC LIMIT 1",
                [&cutoff],
                |row| {
                    Ok(ActiveEntity {
                        entity_type: row.get(0)?,
                        entity_id: row.get(1)?,
                        event_count: row.get::<_, i64>(2)? as usize,
                    })
                },
            )
            .optional()
            .unwrap();

        assert!(result.is_none());
    }

    // --- session_window tests ---

    #[test]
    fn session_window_detects_gap() {
        let db = open_memory_database().unwrap();
        let conn = db.connection();
        let now = Utc::now();

        // Old cluster: 2 hours ago
        let old1 = (now - Duration::hours(2)).to_rfc3339();
        let old2 = (now - Duration::hours(2) + Duration::minutes(5)).to_rfc3339();
        insert_event(conn, "e1", &old1, "command", None, None, "old cmd 1");
        insert_event(conn, "e2", &old2, "command", None, None, "old cmd 2");

        // Recent cluster: 10 min ago (gap > 30 min from old cluster)
        let recent1 = (now - Duration::minutes(10)).to_rfc3339();
        let recent2 = (now - Duration::minutes(5)).to_rfc3339();
        let recent3 = (now - Duration::minutes(1)).to_rfc3339();
        insert_event(conn, "e3", &recent1, "command", None, None, "cmd 1");
        insert_event(conn, "e4", &recent2, "command", None, None, "cmd 2");
        insert_event(conn, "e5", &recent3, "command", None, None, "cmd 3");

        // Test using in-memory DB directly
        let cutoff = (Utc::now() - Duration::hours(24)).to_rfc3339();
        let mut stmt = conn
            .prepare(
                "SELECT timestamp FROM agent_events
                 WHERE timestamp > ?1
                 ORDER BY timestamp ASC",
            )
            .unwrap();

        let timestamps: Vec<DateTime<Utc>> = stmt
            .query_map([&cutoff], |row| {
                let ts: String = row.get(0)?;
                Ok(ts)
            })
            .unwrap()
            .filter_map(|r| r.ok())
            .filter_map(|ts| {
                DateTime::parse_from_rfc3339(&ts)
                    .ok()
                    .map(|dt| dt.with_timezone(&Utc))
            })
            .collect();

        assert_eq!(timestamps.len(), 5);

        let mut session_start_idx = 0;
        for i in 1..timestamps.len() {
            if timestamps[i] - timestamps[i - 1] > Duration::minutes(30) {
                session_start_idx = i;
            }
        }

        // Session should start at the recent cluster (index 2)
        assert_eq!(session_start_idx, 2);
        let count = (timestamps.len() - session_start_idx) as i64;
        assert_eq!(count, 3);
    }

    #[test]
    fn session_window_no_gap_spans_all() {
        let db = open_memory_database().unwrap();
        let conn = db.connection();
        let now = Utc::now();

        // All events within 10 minutes — no gap
        let t1 = (now - Duration::minutes(10)).to_rfc3339();
        let t2 = (now - Duration::minutes(5)).to_rfc3339();
        let t3 = (now - Duration::minutes(1)).to_rfc3339();
        insert_event(conn, "e1", &t1, "command", None, None, "cmd 1");
        insert_event(conn, "e2", &t2, "command", None, None, "cmd 2");
        insert_event(conn, "e3", &t3, "command", None, None, "cmd 3");

        let cutoff = (Utc::now() - Duration::hours(24)).to_rfc3339();
        let mut stmt = conn
            .prepare(
                "SELECT timestamp FROM agent_events
                 WHERE timestamp > ?1
                 ORDER BY timestamp ASC",
            )
            .unwrap();

        let timestamps: Vec<DateTime<Utc>> = stmt
            .query_map([&cutoff], |row| {
                let ts: String = row.get(0)?;
                Ok(ts)
            })
            .unwrap()
            .filter_map(|r| r.ok())
            .filter_map(|ts| {
                DateTime::parse_from_rfc3339(&ts)
                    .ok()
                    .map(|dt| dt.with_timezone(&Utc))
            })
            .collect();

        // No gap > 30 min, so session_start_idx stays at 0
        let mut session_start_idx = 0;
        for i in 1..timestamps.len() {
            if timestamps[i] - timestamps[i - 1] > Duration::minutes(30) {
                session_start_idx = i;
            }
        }

        assert_eq!(session_start_idx, 0);
        let count = (timestamps.len() - session_start_idx) as i64;
        assert_eq!(count, 3);
    }

    // --- recent_file_areas tests ---

    #[test]
    fn recent_file_areas_returns_paths_with_counts() {
        let db = open_memory_database().unwrap();
        let conn = db.connection();
        let recent = (Utc::now() - Duration::minutes(5)).to_rfc3339();

        insert_event(
            conn,
            "e1",
            &recent,
            "file_save",
            None,
            None,
            "file saved: src/main.rs",
        );
        insert_event(
            conn,
            "e2",
            &recent,
            "file_save",
            None,
            None,
            "file saved: src/main.rs",
        );
        insert_event(
            conn,
            "e3",
            &recent,
            "file_save",
            None,
            None,
            "file saved: src/lib.rs",
        );

        let cutoff = (Utc::now() - Duration::minutes(15)).to_rfc3339();
        let mut stmt = conn
            .prepare(
                "SELECT summary, COUNT(*) as cnt
                 FROM agent_events
                 WHERE event_type = 'file_save' AND timestamp > ?1
                 GROUP BY summary
                 ORDER BY cnt DESC LIMIT 10",
            )
            .unwrap();

        let areas: Vec<RecentFileArea> = stmt
            .query_map([&cutoff], |row| {
                let summary: String = row.get(0)?;
                let count: i64 = row.get(1)?;
                Ok((summary, count))
            })
            .unwrap()
            .filter_map(|r| r.ok())
            .filter_map(|(summary, count)| {
                let path = summary.strip_prefix("file saved: ")?;
                Some(RecentFileArea {
                    file_path: path.to_string(),
                    save_count: count as usize,
                })
            })
            .collect();

        assert_eq!(areas.len(), 2);
        assert_eq!(areas[0].file_path, "src/main.rs");
        assert_eq!(areas[0].save_count, 2);
        assert_eq!(areas[1].file_path, "src/lib.rs");
        assert_eq!(areas[1].save_count, 1);
    }

    #[test]
    fn recent_file_areas_empty_when_no_saves() {
        let db = open_memory_database().unwrap();
        let conn = db.connection();
        let recent = (Utc::now() - Duration::minutes(5)).to_rfc3339();

        // Only command events, no file_save
        insert_event(conn, "e1", &recent, "command", None, None, "task list");

        let cutoff = (Utc::now() - Duration::minutes(15)).to_rfc3339();
        let mut stmt = conn
            .prepare(
                "SELECT summary, COUNT(*) as cnt
                 FROM agent_events
                 WHERE event_type = 'file_save' AND timestamp > ?1
                 GROUP BY summary
                 ORDER BY cnt DESC LIMIT 10",
            )
            .unwrap();

        let areas: Vec<RecentFileArea> = stmt
            .query_map([&cutoff], |row| {
                let summary: String = row.get(0)?;
                let count: i64 = row.get(1)?;
                Ok((summary, count))
            })
            .unwrap()
            .filter_map(|r| r.ok())
            .filter_map(|(summary, count)| {
                let path = summary.strip_prefix("file saved: ")?;
                Some(RecentFileArea {
                    file_path: path.to_string(),
                    save_count: count as usize,
                })
            })
            .collect();

        assert!(areas.is_empty());
    }

    // ── Integration tests that call the real public functions ────────

    /// Create a temp workspace with an initialized DB and return (tempdir, root_path).
    fn setup_workspace() -> (tempfile::TempDir, std::path::PathBuf) {
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = tmp.path().to_path_buf();
        let cache = root.join(".cache");
        std::fs::create_dir_all(&cache).expect("create .cache");
        let _db = open_database(root.join(SQLITE_DB_PATH)).expect("init db");
        (tmp, root)
    }

    fn insert_event_at(
        root: &Path,
        text_id: &str,
        ts: &str,
        etype: &str,
        ent_type: Option<&str>,
        ent_id: Option<&str>,
        summary: &str,
    ) {
        let db = open_database(root.join(SQLITE_DB_PATH)).unwrap();
        db.connection().execute(
            "INSERT INTO agent_events (text_id, timestamp, event_type, entity_type, entity_id, summary)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![text_id, ts, etype, ent_type, ent_id, summary],
        ).unwrap();
    }

    #[test]
    fn integration_active_entity_returns_most_frequent() {
        let (_tmp, root) = setup_workspace();
        let now = Utc::now();
        let recent = (now - Duration::minutes(2)).to_rfc3339();
        insert_event_at(
            &root,
            "e1",
            &recent,
            "command",
            Some("goal"),
            Some("g1"),
            "s",
        );
        insert_event_at(
            &root,
            "e2",
            &recent,
            "command",
            Some("goal"),
            Some("g1"),
            "s",
        );
        insert_event_at(
            &root,
            "e3",
            &recent,
            "command",
            Some("task"),
            Some("t1"),
            "s",
        );
        let result = active_entity(&root);
        assert!(result.is_some());
        let ae = result.unwrap();
        assert_eq!(ae.entity_type, "goal");
        assert_eq!(ae.entity_id, "g1");
        assert_eq!(ae.event_count, 2);
    }

    #[test]
    fn integration_active_entity_none_when_old() {
        let (_tmp, root) = setup_workspace();
        let old = (Utc::now() - Duration::minutes(20)).to_rfc3339();
        insert_event_at(&root, "e1", &old, "command", Some("goal"), Some("g1"), "s");
        assert!(active_entity(&root).is_none());
    }

    #[test]
    fn integration_session_window_detects_gap() {
        let (_tmp, root) = setup_workspace();
        let now = Utc::now();
        // Old event (2 hours ago) then recent events
        let old = (now - Duration::hours(2)).to_rfc3339();
        let recent1 = (now - Duration::minutes(5)).to_rfc3339();
        let recent2 = (now - Duration::minutes(1)).to_rfc3339();
        insert_event_at(&root, "e1", &old, "command", None, None, "old");
        insert_event_at(&root, "e2", &recent1, "command", None, None, "r1");
        insert_event_at(&root, "e3", &recent2, "command", None, None, "r2");
        let sw = session_window(&root).expect("should find session");
        assert_eq!(sw.event_count, 2); // only events after the gap
    }

    #[test]
    fn integration_recent_file_areas_strips_prefix() {
        let (_tmp, root) = setup_workspace();
        let recent = (Utc::now() - Duration::minutes(2)).to_rfc3339();
        insert_event_at(
            &root,
            "e1",
            &recent,
            "file_save",
            None,
            None,
            "file saved: src/main.rs",
        );
        insert_event_at(
            &root,
            "e2",
            &recent,
            "file_save",
            None,
            None,
            "file saved: src/main.rs",
        );
        insert_event_at(
            &root,
            "e3",
            &recent,
            "file_save",
            None,
            None,
            "file saved: src/lib.rs",
        );
        let areas = recent_file_areas(&root);
        assert_eq!(areas.len(), 2);
        assert_eq!(areas[0].file_path, "src/main.rs");
        assert_eq!(areas[0].save_count, 2);
    }

    // --- top_directory tests ---

    #[test]
    fn top_directory_extracts_first_component() {
        assert_eq!(top_directory("src/auth/mod.rs"), Some("src"));
        assert_eq!(top_directory("crates/foo/lib.rs"), Some("crates"));
    }

    #[test]
    fn top_directory_returns_none_for_root_file() {
        assert_eq!(top_directory("Cargo.toml"), None);
        assert_eq!(top_directory("README.md"), None);
    }

    // --- detect_drift tests ---

    #[test]
    fn detect_drift_flags_when_majority_outside_scope() {
        let scope = vec!["crates".to_string()];
        let recent = vec![
            RecentFileArea {
                file_path: "src/main.rs".to_string(),
                save_count: 3,
            },
            RecentFileArea {
                file_path: "src/lib.rs".to_string(),
                save_count: 2,
            },
            RecentFileArea {
                file_path: "crates/foo/mod.rs".to_string(),
                save_count: 1,
            },
        ];
        let drift = detect_drift(&recent, &scope);
        assert!(drift.is_some());
        let d = drift.unwrap();
        assert_eq!(d.outside_dirs, vec!["src".to_string()]);
    }

    #[test]
    fn detect_drift_no_drift_when_all_in_scope() {
        let scope = vec!["src".to_string(), "crates".to_string()];
        let recent = vec![
            RecentFileArea {
                file_path: "src/main.rs".to_string(),
                save_count: 3,
            },
            RecentFileArea {
                file_path: "crates/foo/mod.rs".to_string(),
                save_count: 2,
            },
        ];
        assert!(detect_drift(&recent, &scope).is_none());
    }

    #[test]
    fn detect_drift_no_drift_on_empty_scope() {
        let recent = vec![RecentFileArea {
            file_path: "src/main.rs".to_string(),
            save_count: 1,
        }];
        assert!(detect_drift(&recent, &[]).is_none());
    }

    #[test]
    fn detect_drift_no_drift_on_empty_recent() {
        let scope = vec!["src".to_string()];
        assert!(detect_drift(&[], &scope).is_none());
    }

    #[test]
    fn detect_drift_root_files_not_counted_as_drift() {
        let scope = vec!["src".to_string()];
        let recent = vec![
            RecentFileArea {
                file_path: "Cargo.toml".to_string(),
                save_count: 5,
            },
            RecentFileArea {
                file_path: "src/main.rs".to_string(),
                save_count: 1,
            },
        ];
        assert!(detect_drift(&recent, &scope).is_none());
    }

    // --- infer_entity_scope integration test ---

    #[test]
    fn integration_infer_entity_scope_returns_top_dirs() {
        let (_tmp, root) = setup_workspace();
        let now = Utc::now();
        // All events within 10 min — single session, no gap
        let t1 = (now - Duration::minutes(8)).to_rfc3339();
        let t2 = (now - Duration::minutes(6)).to_rfc3339();
        let t3 = (now - Duration::minutes(4)).to_rfc3339();
        let t4 = (now - Duration::minutes(2)).to_rfc3339();
        // A command event to anchor the session
        insert_event_at(&root, "e0", &t1, "command", None, None, "task list");
        // File saves in crates/ and src/
        insert_event_at(
            &root,
            "e1",
            &t2,
            "file_save",
            None,
            None,
            "file saved: crates/foo/lib.rs",
        );
        insert_event_at(
            &root,
            "e2",
            &t3,
            "file_save",
            None,
            None,
            "file saved: crates/bar/mod.rs",
        );
        insert_event_at(
            &root,
            "e3",
            &t4,
            "file_save",
            None,
            None,
            "file saved: src/main.rs",
        );

        let scope = infer_entity_scope(&root);
        assert!(!scope.is_empty());
        assert!(scope.contains(&"crates".to_string()));
        assert!(scope.contains(&"src".to_string()));
    }
}
