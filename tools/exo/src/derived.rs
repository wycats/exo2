use std::{collections::HashMap, path::Path};

use crate::context::SqliteLoader;
#[cfg(test)]
use crate::context::{SQLITE_DB_PATH, SqliteWriter};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DerivedGoalStatus {
    pub tasks_complete: bool,
    pub pending_count: usize,
    pub completed_count: usize,
    pub reason: String,
}

#[derive(Debug)]
pub struct DeriveContext<'a> {
    pub root: &'a Path,
    rfc_stages: HashMap<String, u8>,
}

impl<'a> DeriveContext<'a> {
    pub fn load(root: &'a Path) -> Self {
        let mut rfc_stages = HashMap::new();

        let project = crate::project::Project::resolve(root).ok();
        if let Ok(rfcs) = crate::rfc::load_effective_rfcs(root, project.as_ref()) {
            for effective in rfcs {
                let r = effective.record;
                rfc_stages.insert(format!("{:05}", r.rfc_number), r.stage);
            }
        } else {
            let db_path = crate::context::db_path(root, project.as_ref());
            if let Ok(loader) = SqliteLoader::open(&db_path)
                && let Ok(rfcs) = loader.load_rfcs()
            {
                for r in rfcs {
                    rfc_stages.insert(format!("{:05}", r.rfc_number), r.stage);
                }
            }
        }

        Self { root, rfc_stages }
    }

    pub fn rfc_stage(&self, id: &str) -> Option<u8> {
        self.rfc_stages.get(id).copied()
    }
}

#[derive(Debug, Clone, Copy)]
pub enum Evidence {
    /// A workspace-relative path exists.
    FileExists { path: &'static str },

    /// At least one file matched by the workspace-relative glob contains the given substring.
    ///
    /// Example glob: "packages/exosuit-vscode/src/**/*.ts"
    TextSearch {
        glob: &'static str,
        needle: &'static str,
    },

    /// RFC is at least a given stage (loaded from docs/rfcs).
    RfcStageAtLeast { rfc: &'static str, min_stage: u8 },
}

#[derive(Debug, Clone, Copy)]
pub struct DerivationRule {
    pub phase_id: &'static str,
    pub task_id: &'static str,
    pub derived_status: &'static str,
    pub all_of: &'static [Evidence],
}

static PHASE_67_SEED_RFCS_EVIDENCE: [Evidence; 2] = [
    Evidence::RfcStageAtLeast {
        rfc: "00127",
        min_stage: 2,
    },
    Evidence::RfcStageAtLeast {
        rfc: "00133",
        min_stage: 1,
    },
];

static RULES: [DerivationRule; 1] = [DerivationRule {
    phase_id: "phase-67-machine-channel-projection",
    task_id: "seed-rfcs",
    derived_status: "completed",
    all_of: &PHASE_67_SEED_RFCS_EVIDENCE,
}];

fn evidence_satisfied(ctx: &DeriveContext<'_>, e: Evidence) -> bool {
    match e {
        Evidence::FileExists { path } => ctx.root.join(path).exists(),
        Evidence::TextSearch { glob, needle } => text_search_any(ctx.root, glob, needle),
        Evidence::RfcStageAtLeast { rfc, min_stage } => {
            ctx.rfc_stage(rfc).is_some_and(|s| s >= min_stage)
        }
    }
}

fn describe_evidence(ctx: &DeriveContext<'_>, e: Evidence) -> String {
    match e {
        Evidence::FileExists { path } => {
            format!("file exists: {path}")
        }
        Evidence::TextSearch { glob, needle } => {
            // Keep needles short-ish in the reason output.
            let needle = if needle.len() > 80 {
                format!("{}…", &needle[..80])
            } else {
                needle.to_string()
            };
            format!("text match: {glob} contains \"{needle}\"")
        }
        Evidence::RfcStageAtLeast { rfc, min_stage } => {
            let stage = ctx
                .rfc_stage(rfc)
                .map_or_else(|| "?".to_string(), |s| s.to_string());
            format!("RFC {rfc} stage {stage} (>= {min_stage})")
        }
    }
}

fn text_search_any(root: &Path, pattern: &str, needle: &str) -> bool {
    let pattern = root.join(pattern).to_string_lossy().to_string();

    let Ok(paths) = glob::glob(&pattern) else {
        return false;
    };

    for entry in paths.flatten() {
        if !entry.is_file() {
            continue;
        }
        let Ok(content) = std::fs::read_to_string(&entry) else {
            continue;
        };
        if content.contains(needle) {
            return true;
        }
    }

    false
}

pub fn derive_goal_status(
    ctx: &DeriveContext<'_>,
    phase_id: &str,
    task_id: &str,
) -> Option<DerivedGoalStatus> {
    for rule in RULES {
        if rule.phase_id != phase_id || rule.task_id != task_id {
            continue;
        }

        if !rule
            .all_of
            .iter()
            .copied()
            .all(|e| evidence_satisfied(ctx, e))
        {
            continue;
        }

        let reasons = rule
            .all_of
            .iter()
            .copied()
            .map(|e| describe_evidence(ctx, e))
            .collect::<Vec<_>>()
            .join(", ");

        return Some(DerivedGoalStatus {
            tasks_complete: true,
            pending_count: 0,
            completed_count: 1,
            reason: format!("Derived from evidence: {reasons}."),
        });
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn seed_rfcs_derives_completed_when_rfc_stages_met() {
        let temp_res = tempfile::tempdir();
        assert!(temp_res.is_ok(), "failed to create tempdir");
        let Ok(temp) = temp_res else {
            return;
        };
        let root = temp.path();

        let cache_res = fs::create_dir_all(root.join(".cache"));
        assert!(cache_res.is_ok(), "failed to create .cache dir");
        if cache_res.is_err() {
            return;
        }

        let writer_res = SqliteWriter::open(root.join(SQLITE_DB_PATH));
        assert!(writer_res.is_ok(), "failed to open sqlite writer");
        let Ok(writer) = writer_res else {
            return;
        };
        let upsert_127 = writer.upsert_rfc(
            "rfc_0127",
            127,
            "Machine Channel",
            2,
            "active",
            Some("Test"),
            "machine-channel",
            "docs/rfcs/stage-2/0127-machine-channel.md",
            None,
            None,
            None,
            None,
            None,
        );
        assert!(upsert_127.is_ok(), "failed to seed RFC 0127");
        if upsert_127.is_err() {
            return;
        }

        let upsert_133 = writer.upsert_rfc(
            "rfc_0133",
            133,
            "Impl Plan",
            1,
            "active",
            Some("Test"),
            "impl-plan",
            "docs/rfcs/stage-1/0133-impl-plan.md",
            None,
            None,
            None,
            None,
            None,
        );
        assert!(upsert_133.is_ok(), "failed to seed RFC 0133");
        if upsert_133.is_err() {
            return;
        }

        let ctx = DeriveContext::load(root);
        let derived_opt =
            derive_goal_status(&ctx, "phase-67-machine-channel-projection", "seed-rfcs");
        assert!(derived_opt.is_some(), "should derive");
        let Some(derived) = derived_opt else {
            return;
        };
        assert!(derived.tasks_complete);
        assert_eq!(derived.pending_count, 0);
        assert_eq!(derived.completed_count, 1);
        assert!(derived.reason.contains("0127"));
        assert!(derived.reason.contains("0133"));
    }
}
