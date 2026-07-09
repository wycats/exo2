use crate::ExoResult;
use crate::project::Project;
use std::collections::HashMap;
use std::path::Path;

#[derive(Debug, Clone)]
pub struct RfcIndexEntry {
    pub id: String,
    pub stage: u8,
    pub title: String,
}

pub fn index_rfcs(root: &Path) -> ExoResult<HashMap<String, RfcIndexEntry>> {
    let project = Project::resolve(root).ok();
    index_rfcs_with_project(root, project.as_ref())
}

pub fn index_rfcs_with_project(
    root: &Path,
    project: Option<&Project>,
) -> ExoResult<HashMap<String, RfcIndexEntry>> {
    let db_path = crate::context::db_path(root, project);
    if !db_path.exists() {
        return Ok(HashMap::new());
    }

    let rfcs = crate::rfc::load_effective_rfcs(root, project)?;
    let mut entries = HashMap::new();

    for effective in rfcs {
        let rfc = effective.record;
        entries.insert(
            format!("{:05}", rfc.rfc_number),
            RfcIndexEntry {
                id: format!("{:05}", rfc.rfc_number),
                stage: rfc.stage,
                title: rfc.title,
            },
        );
    }

    Ok(entries)
}
