//! Plugin to install global prompts.
//!
//! This plugin installs/updates prompts to ~/.config/Code/User/prompts/exo/

use crate::ExoResult;
use crate::context::AgentContext;
use crate::templates;
use crate::upgrade::{Severity, UpgradePlugin, UpgradeReport, UpgradeStatus};

/// Installs global prompts to the user's VS Code prompts directory.
///
/// Path: `~/.config/Code/User/prompts/exo/`
///
/// # Severity
///
/// **Info** - Global prompts are nice-to-have.
#[derive(Debug, Clone, Copy)]
pub struct InstallGlobalPromptsPlugin;

impl UpgradePlugin for InstallGlobalPromptsPlugin {
    fn id(&self) -> &str {
        "install-global-prompts-v1"
    }

    fn description(&self) -> &str {
        "Installs global prompts to ~/.config/Code/User/prompts/exo/"
    }

    fn severity(&self) -> Severity {
        Severity::Info
    }

    fn is_needed(&self, _context: &AgentContext) -> ExoResult<UpgradeStatus> {
        // Always report as needed since prompts may have been updated
        Ok(UpgradeStatus::info("Global prompts may need updating"))
    }

    fn apply(&self, _context: &mut AgentContext) -> ExoResult<UpgradeReport> {
        match templates::install_global_prompts() {
            Ok(written) if written > 0 => Ok(UpgradeReport::with_changes(
                self.id(),
                vec![format!("Installed/updated {written} global prompts")],
            )),
            Ok(_) => Ok(UpgradeReport::no_changes(self.id())),
            Err(e) => {
                // Non-fatal: just warn (user might not have VS Code)
                Ok(UpgradeReport::no_changes(self.id())
                    .with_warning(format!("Could not install global prompts: {e}")))
            }
        }
    }

    fn verify(&self, _context: &AgentContext) -> ExoResult<()> {
        // Verification is optional for global prompts
        Ok(())
    }
}
