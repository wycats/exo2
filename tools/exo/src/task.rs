#![allow(clippy::redundant_pub_crate)]

//! Task manipulation functions.
//!
//! # Output Boundary (RFC 00234)
//!
//! Functions in this module that may be called from the command layer return
//! messages as `ExoResult<String>` instead of printing directly. This prevents
//! stdout pollution when called via the machine channel (JSON server).

#![deny(clippy::print_stdout, clippy::print_stderr)]

use crate::ExoResult;
use crate::context::{AgentContext, SqliteLoader};
use anyhow::anyhow;
use std::collections::HashMap;
use std::path::Path;

fn list_active_phase_tasks_via_sqlite(
    root: &Path,
) -> ExoResult<Option<Vec<(String, String, String)>>> {
    let ctx = AgentContext::load(root.to_path_buf()).ok();
    let Some(ctx) = ctx.as_ref() else {
        return Ok(None);
    };
    list_active_phase_tasks_from_context(ctx)
}

fn list_active_phase_tasks_from_context(
    ctx: &AgentContext,
) -> ExoResult<Option<Vec<(String, String, String)>>> {
    let workspace_root = ctx.workspace_root_key();
    let db_path = crate::context::db_path(&ctx.root, ctx.project.as_ref());
    if !db_path.exists() {
        return Ok(None);
    }
    let loader = SqliteLoader::open(&db_path)?;
    let tasks = loader.list_active_phase_tasks_for_workspace(workspace_root.as_deref())?;
    Ok(if tasks.is_empty() { None } else { Some(tasks) })
}

fn load_active_phase_goal_statuses(root: &Path) -> HashMap<String, String> {
    let Ok(ctx) = AgentContext::load(root.to_path_buf()) else {
        return HashMap::new();
    };

    let Ok(Some(active_phase)) = ctx.find_workspace_active_phase() else {
        return HashMap::new();
    };

    let mut statuses = HashMap::new();
    for goal in &active_phase.phase.goals {
        statuses.insert(goal.id.clone(), goal.status.clone());
        for alias in &goal.aliases {
            statuses.insert(alias.clone(), goal.status.clone());
        }
    }

    statuses
}

fn overlay_abandoned_status(task_status: &str, goal_status: Option<&String>) -> String {
    if task_status == "pending" && goal_status.is_some_and(|s| s == "abandoned") {
        "abandoned".to_string()
    } else {
        task_status.to_string()
    }
}

// Dead TOML fallback removed (~80 lines). Was unreachable: StorageBackend is always Sqlite.

/// List active-phase tasks from the canonical `SQLite` store.
pub(crate) fn list_active_phase_tasks_only(
    root: &Path,
) -> ExoResult<Vec<(String, String, String)>> {
    Ok(list_active_phase_tasks_via_sqlite(root)?.unwrap_or_default())
}

/// List active-phase tasks for execution-oriented consumers.
///
/// This is now a SQLite-backed read over canonical active-phase tasks.
pub(crate) fn list_execution_tasks(root: &Path) -> ExoResult<Vec<(String, String, String)>> {
    Ok(list_active_phase_tasks_via_sqlite(root)?.unwrap_or_default())
}
pub(crate) fn list_tasks(root: &Path) -> ExoResult<Vec<(String, String, String)>> {
    if let Some(tasks) = list_active_phase_tasks_via_sqlite(root)? {
        return Ok(tasks);
    }

    let ctx = AgentContext::load(root.to_path_buf())?;
    if ctx.find_workspace_active_phase()?.is_none() {
        return Err(anyhow!(
            "No active phase found. Use `exo phase start <id>` to start one."
        ));
    }

    Ok(Vec::new())
}

/// List tasks using the context already loaded for the current command.
pub(crate) fn list_tasks_for_context(
    context: &AgentContext,
) -> ExoResult<Vec<(String, String, String)>> {
    if let Some(tasks) = list_active_phase_tasks_from_context(context)? {
        return Ok(tasks);
    }

    if context.find_workspace_active_phase()?.is_none() {
        return Err(anyhow!(
            "No active phase found. Use `exo phase start <id>` to start one."
        ));
    }

    Ok(Vec::new())
}

#[derive(Debug, Clone)]
pub(crate) struct TaskListItem {
    pub id: String,
    pub label: String,
    pub status: String,
}

#[derive(Debug, Clone)]
pub(crate) struct TaskListGroup {
    pub goal_id: String,
    pub goal_label: String,
    pub goal_status: String,
    pub tasks: Vec<TaskListItem>,
}

pub(crate) fn list_task_groups(root: &Path) -> ExoResult<Vec<TaskListGroup>> {
    let ctx = AgentContext::load(root.to_path_buf())?;
    let Some(active_phase) = ctx.find_workspace_active_phase()? else {
        return Err(anyhow!(
            "No active phase found. Use `exo phase start <id>` to start one."
        ));
    };

    let workspace_root = ctx.workspace_root_key();
    let db_path = crate::context::db_path(root, ctx.project.as_ref());
    if !db_path.exists() {
        return Ok(Vec::new());
    }

    let loader = SqliteLoader::open(&db_path)?;
    let task_rows = loader.list_active_phase_tasks_for_workspace(workspace_root.as_deref())?;
    let goal_statuses = load_active_phase_goal_statuses(root);

    let mut groups = Vec::new();
    let mut group_index: HashMap<String, usize> = HashMap::new();

    for goal in &active_phase.phase.goals {
        let index = groups.len();
        groups.push(TaskListGroup {
            goal_id: goal.id.clone(),
            goal_label: goal.label.clone(),
            goal_status: goal.status.clone(),
            tasks: Vec::new(),
        });
        group_index.insert(goal.id.clone(), index);
        for alias in &goal.aliases {
            group_index.insert(alias.clone(), index);
        }
    }

    for (task_id, title, status) in task_rows {
        let goal_part = task_id.split("::").next().unwrap_or("");
        let Some(index) = group_index.get(goal_part).copied() else {
            continue;
        };

        let goal_id = groups[index].goal_id.clone();
        let status = overlay_abandoned_status(&status, goal_statuses.get(&goal_id));

        groups[index].tasks.push(TaskListItem {
            id: task_id,
            label: title,
            status,
        });
    }

    Ok(groups)
}
