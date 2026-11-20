//! Which automation clients the owner approved, for which scopes and workspaces.
//!
//! Grants live in `automation-clients.json` in the app root, readable only by
//! the owner. The desktop instance re-reads the file before every call, so an
//! approval, pause or revocation made in Rodtty or with `rodtty automation`
//! applies to the next request.
use super::{ErrorCode, ToolError};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::Path;

pub const FILE: &str = "automation-clients.json";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Scope {
    /// Read workspace, terminal, editor, Git and browser state.
    Read,
    /// Create, launch, type into, interrupt and close terminal sessions.
    Terminal,
    /// Virtual pointer and keyboard input to Rodtty surfaces.
    Input,
    /// Switch workspaces, show the window and focus panes.
    Workspace,
    /// Open, edit and save editor buffers.
    Editor,
    /// Create, navigate and operate embedded browser targets.
    Browser,
    /// Screenshots and browser recordings.
    Capture,
}

impl Scope {
    pub const ALL: [Scope; 7] = [
        Scope::Read,
        Scope::Terminal,
        Scope::Input,
        Scope::Workspace,
        Scope::Editor,
        Scope::Browser,
        Scope::Capture,
    ];
    pub fn key(self) -> &'static str {
        match self {
            Scope::Read => "read",
            Scope::Terminal => "terminal",
            Scope::Input => "input",
            Scope::Workspace => "workspace",
            Scope::Editor => "editor",
            Scope::Browser => "browser",
            Scope::Capture => "capture",
        }
    }
    pub fn parse(key: &str) -> Option<Scope> {
        Scope::ALL.into_iter().find(|s| s.key() == key)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    /// Waiting for the owner. No state is readable.
    Pending,
    Approved,
    /// Temporarily stopped by the owner; queued input is cancelled.
    Paused,
    Revoked,
}

/// Every workspace, including ones created later.
pub const ALL_WORKSPACES: &str = "*";

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ClientGrant {
    pub id: String,
    /// The name the MCP client reported, or `--client-name`.
    pub name: String,
    pub status: Status,
    #[serde(default)]
    pub scopes: Vec<Scope>,
    /// Workspace IDs, or `*`.
    #[serde(default)]
    pub workspaces: Vec<String>,
    #[serde(default)]
    pub requested_at: u64,
    #[serde(default)]
    pub updated_at: u64,
}

impl ClientGrant {
    pub fn allows_workspace(&self, workspace: &str) -> bool {
        self.workspaces
            .iter()
            .any(|w| w == ALL_WORKSPACES || w == workspace)
    }

    /// Whether this client may use `scope`, and on `workspace` when the
    /// target belongs to one.
    pub fn check(&self, scope: Option<Scope>, workspace: Option<&str>) -> Result<(), ToolError> {
        match self.status {
            Status::Pending => {
                return Err(ToolError::new(
                    ErrorCode::AuthorizationRequired,
                    format!(
                        "Waiting for the owner to approve client {} ({}). Approve it from Rodtty's command palette (Automation: Approve Pending Client) or run `rodtty automation approve {}`.",
                        self.name, self.id, self.id
                    ),
                ));
            }
            Status::Paused => {
                return Err(ToolError::new(
                    ErrorCode::Paused,
                    "The owner paused this client",
                ));
            }
            Status::Revoked => {
                return Err(ToolError::new(
                    ErrorCode::Revoked,
                    "The owner revoked this client",
                ));
            }
            Status::Approved => {}
        }
        if let Some(scope) = scope
            && !self.scopes.contains(&scope)
        {
            return Err(ToolError::new(
                ErrorCode::ScopeDenied,
                format!("This client's grant doesn't include the `{}` scope", scope.key()),
            ));
        }
        if let Some(workspace) = workspace
            && !self.allows_workspace(workspace)
        {
            return Err(ToolError::new(
                ErrorCode::WorkspaceDenied,
                "This client's grant doesn't include that workspace",
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Grants {
    #[serde(default)]
    pub clients: Vec<ClientGrant>,
}

fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

impl Grants {
    pub fn load(root: &Path) -> Result<Self> {
        match std::fs::read(root.join(FILE)) {
            Ok(bytes) => serde_json::from_slice(&bytes).context("read automation-clients.json"),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(error) => Err(error).context("read automation-clients.json"),
        }
    }

    pub fn save(&self, root: &Path) -> Result<()> {
        use std::io::Write;
        std::fs::create_dir_all(root)?;
        let mut file = tempfile::NamedTempFile::new_in(root)?;
        file.write_all(&serde_json::to_vec_pretty(self)?)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            file.as_file()
                .set_permissions(std::fs::Permissions::from_mode(0o600))?;
        }
        file.persist(root.join(FILE)).map_err(|e| e.error)?;
        Ok(())
    }

    pub fn get(&self, id: &str) -> Option<&ClientGrant> {
        self.clients.iter().find(|c| c.id == id)
    }

    fn get_mut(&mut self, id: &str) -> Result<&mut ClientGrant> {
        self.clients
            .iter_mut()
            .find(|c| c.id == id || c.name == id)
            .with_context(|| format!("no automation client {id}"))
    }

    /// The grant for a client named `name`, or a new pending request for it.
    pub fn request(&mut self, name: &str) -> (ClientGrant, bool) {
        if let Some(existing) = self.clients.iter().find(|c| c.name == name) {
            return (existing.clone(), false);
        }
        let grant = ClientGrant {
            id: format!("cl_{}", super::token()),
            name: name.to_owned(),
            status: Status::Pending,
            scopes: Vec::new(),
            workspaces: Vec::new(),
            requested_at: now(),
            updated_at: now(),
        };
        self.clients.push(grant.clone());
        (grant, true)
    }

    pub fn approve(&mut self, id: &str, scopes: &[Scope], workspaces: &[String]) -> Result<()> {
        let grant = self.get_mut(id)?;
        grant.status = Status::Approved;
        grant.scopes = scopes.to_vec();
        grant.scopes.sort();
        grant.scopes.dedup();
        grant.workspaces = workspaces.to_vec();
        grant.updated_at = now();
        Ok(())
    }

    pub fn set_status(&mut self, id: &str, status: Status) -> Result<()> {
        let grant = self.get_mut(id)?;
        grant.status = status;
        grant.updated_at = now();
        Ok(())
    }

    /// Forget a client entirely; its next connection asks for approval again.
    pub fn forget(&mut self, id: &str) -> bool {
        let before = self.clients.len();
        self.clients.retain(|c| c.id != id && c.name != id);
        self.clients.len() != before
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_client_needs_approval_scope_and_workspace() {
        let home = crate::test_support::tempdir().unwrap();
        let mut grants = Grants::default();
        let (pending, created) = grants.request("codex");
        assert!(created);
        assert_eq!(
            pending.check(Some(Scope::Read), None).unwrap_err().code,
            ErrorCode::AuthorizationRequired
        );
        assert!(!grants.request("codex").1, "one request per client");
        grants
            .approve(&pending.id, &[Scope::Read, Scope::Terminal], &["ws_a".into()])
            .unwrap();
        grants.save(home.path()).unwrap();
        let grants = Grants::load(home.path()).unwrap();
        let grant = grants.get(&pending.id).unwrap();
        assert!(grant.check(Some(Scope::Terminal), Some("ws_a")).is_ok());
        assert_eq!(
            grant.check(Some(Scope::Read), Some("ws_b")).unwrap_err().code,
            ErrorCode::WorkspaceDenied
        );
        assert_eq!(
            grant.check(Some(Scope::Editor), None).unwrap_err().code,
            ErrorCode::ScopeDenied
        );
        let mut grants = grants;
        grants.set_status(&pending.id, Status::Revoked).unwrap();
        assert_eq!(
            grants.get(&pending.id).unwrap().check(None, None).unwrap_err().code,
            ErrorCode::Revoked
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(home.path().join(FILE))
                .unwrap()
                .permissions()
                .mode();
            assert_eq!(mode & 0o777, 0o600);
        }
    }
}
