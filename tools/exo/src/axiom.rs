use crate::ExoResult;
use crate::project::Project;
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AxiomScope {
    Workflow,
    System,
    Design,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Axiom {
    pub id: String,
    pub principle: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rationale: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub implications: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
}
pub fn list_axioms(root: &Path, scope: &str) -> ExoResult<Vec<Axiom>> {
    list_axioms_with_project(root, None, scope)
}

pub fn list_axioms_with_project(
    root: &Path,
    project: Option<&Project>,
    scope: &str,
) -> ExoResult<Vec<Axiom>> {
    let project = project.cloned().or_else(|| Project::resolve(root).ok());
    let db_path = crate::context::db_path(root, project.as_ref());
    let loader = crate::context::SqliteLoader::open(&db_path)?;
    loader.list_axioms(Some(scope))
}

pub fn add_axiom(root: &Path, scope: &str, axiom: Axiom) -> ExoResult<()> {
    let db_path = crate::context::db_path_resolving_project(root);
    let writer = crate::context::SqliteWriter::open(db_path)?;
    writer.add_axiom(
        &axiom.id,
        scope,
        &axiom.principle,
        axiom.rationale.as_deref(),
        axiom.notes.as_deref(),
        &axiom.implications,
        &axiom.tags,
    )?;
    crate::context::write_sql_dump(root);
    Ok(())
}

pub fn remove_axiom(root: &Path, id: &str) -> ExoResult<()> {
    let db_path = crate::context::db_path_resolving_project(root);
    let writer = crate::context::SqliteWriter::open(db_path)?;
    writer.remove_axiom(id)?;
    crate::context::write_sql_dump(root);
    Ok(())
}
