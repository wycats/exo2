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
    Ok(())
}

pub fn remove_axiom(root: &Path, id: &str) -> ExoResult<()> {
    let db_path = crate::context::db_path_resolving_project(root);
    let writer = crate::context::SqliteWriter::open(db_path)?;
    writer.remove_axiom(id)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn axiom(id: &str) -> Axiom {
        Axiom {
            id: id.to_string(),
            principle: format!("Principle {id}"),
            rationale: None,
            implications: Vec::new(),
            notes: None,
            tags: Vec::new(),
        }
    }

    #[test]
    fn atomic_axiom_mutations_defer_projection_until_post_commit() {
        let temp = tempfile::tempdir().expect("tempdir");
        let root = temp.path();
        let db_path = root.join(crate::context::SQLITE_DB_PATH);
        std::fs::create_dir_all(db_path.parent().expect("database parent"))
            .expect("create database directory");
        drop(exosuit_storage::open_database(&db_path).expect("initialize database"));
        crate::context::SqliteWriter::open(&db_path)
            .expect("open writer")
            .add_axiom(
                "existing",
                "workflow",
                "Existing principle",
                None,
                None,
                &[],
                &[],
            )
            .expect("seed axiom");

        let transaction =
            exosuit_storage::RequestTransaction::begin(&db_path).expect("begin request");
        remove_axiom(root, "existing").expect("remove axiom in request");
        add_axiom(root, "workflow", axiom("new")).expect("add axiom in request");

        assert!(
            !root.join("docs/agent-context/axioms.sql").exists(),
            "axiom commands must leave projection publication to post-commit persistence"
        );
        transaction.rollback().expect("roll back request");

        let axioms = list_axioms(root, "workflow").expect("read rolled-back axioms");
        assert_eq!(axioms.len(), 1);
        assert_eq!(axioms[0].id, "existing");
    }
}
