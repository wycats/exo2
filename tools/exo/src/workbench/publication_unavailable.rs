use super::{WorkbenchEntryBinding, WorkbenchEntryProvider, WorkspaceRegistration};
use anyhow::Result;
use std::net::TcpListener;

#[derive(Debug)]
pub(super) struct LocaldWorkbenchEntryProvider;

impl LocaldWorkbenchEntryProvider {
    pub(super) const fn production() -> Self {
        Self
    }
}

impl WorkbenchEntryProvider for LocaldWorkbenchEntryProvider {
    fn resolve(
        &self,
        _workspace: &WorkspaceRegistration,
        direct_origin: &str,
        _listener: &TcpListener,
        _listener_generation: u64,
        _authorize: &mut dyn FnMut(&WorkbenchEntryBinding) -> Result<()>,
        ensure_started: &mut dyn FnMut() -> Result<()>,
    ) -> Result<WorkbenchEntryBinding> {
        ensure_started()?;
        Ok(WorkbenchEntryBinding::direct(direct_origin.to_string()))
    }
}
