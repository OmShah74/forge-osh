//! McpManager — owns the lifecycle of every configured MCP server.
//!
//! The manager is the single integration point between forge-osh and MCP:
//! - reads server configuration + secrets from `Config` and `KeyStore`
//! - spawns servers, performs handshake, lists their tools
//! - exposes a snapshot API the TUI uses to render status
//! - registers/unregisters tools into a shared `ToolRegistry` at runtime

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::RwLock;

use super::catalog::{self, CatalogEntry};
use super::client::McpClient;
use super::tool_adapter::McpTool;
use crate::config::keyring::KeyStore;
use crate::config::McpServerConfig;
use crate::tools::ToolRegistry;
use crate::types::PermissionLevel;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ServerStatus {
    Disabled,
    Disconnected,
    Connecting,
    Active,
    Error(String),
}

impl ServerStatus {
    pub fn is_active(&self) -> bool {
        matches!(self, ServerStatus::Active)
    }
    pub fn label(&self) -> String {
        match self {
            ServerStatus::Disabled => "disabled".into(),
            ServerStatus::Disconnected => "disconnected".into(),
            ServerStatus::Connecting => "connecting…".into(),
            ServerStatus::Active => "active".into(),
            ServerStatus::Error(e) => format!("error: {}", short(e)),
        }
    }
}

fn short(s: &str) -> String {
    if s.len() > 80 {
        format!("{}…", &s[..80])
    } else {
        s.to_string()
    }
}

#[derive(Debug, Clone)]
pub struct ServerSnapshot {
    pub id: String,
    pub display_name: String,
    pub description: String,
    pub category: String,
    pub enabled: bool,
    pub status: ServerStatus,
    pub tool_count: usize,
    pub server_version: String,
    pub recent_stderr: Vec<String>,
    pub required_secrets: Vec<SecretStatus>,
    pub last_error: Option<String>,
}

#[derive(Debug, Clone)]
pub struct SecretStatus {
    pub key: String,
    pub label: String,
    pub help: String,
    pub required: bool,
    pub present: bool,
    pub source: SecretSource,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SecretSource {
    None,
    Env,
    Stored,
}

struct ServerState {
    /// Snapshot of static descriptors from catalog (or custom config).
    display_name: String,
    description: String,
    category: String,
    command: String,
    args: Vec<String>,
    secret_specs: Vec<catalog::SecretSpec>,
    enabled: bool,
    status: ServerStatus,
    last_error: Option<String>,
    client: Option<Arc<McpClient>>,
    /// Local names of tools registered into the ToolRegistry.
    registered_tool_names: Vec<String>,
    server_version: String,
}

pub struct McpManager {
    /// id → ServerState
    servers: RwLock<HashMap<String, ServerState>>,
    keystore: Arc<tokio::sync::Mutex<KeyStore>>,
    /// Shared tool registry. The registry now uses interior mutability so
    /// MCP tools can be added/removed at runtime as servers connect or
    /// disconnect.
    registry: Arc<ToolRegistry>,
    connect_timeout: Duration,
}

impl McpManager {
    pub fn new(
        keystore: Arc<tokio::sync::Mutex<KeyStore>>,
        registry: Arc<ToolRegistry>,
    ) -> Self {
        Self {
            servers: RwLock::new(HashMap::new()),
            keystore,
            registry,
            connect_timeout: Duration::from_secs(45),
        }
    }

    /// Load all server records from config. Does not yet connect them.
    pub async fn load_from_config(&self, configs: &[McpServerConfig]) {
        let mut map = self.servers.write().await;
        // Always preload every catalog entry as a known server (disabled).
        for entry in catalog::CATALOG {
            map.entry(entry.id.to_string())
                .or_insert_with(|| state_from_catalog(entry, false));
        }
        // Then overlay config — config rows toggle enabled and may override
        // command/args for catalog entries, or define entirely new servers.
        for c in configs {
            let secret_specs = c.secret_specs.clone();
            let entry = map.entry(c.id.clone());
            entry
                .and_modify(|s| {
                    s.enabled = c.enabled;
                    if !c.command.is_empty() {
                        s.command = c.command.clone();
                        s.args = c.args.clone();
                    }
                    if !secret_specs.is_empty() {
                        s.secret_specs = secret_specs.clone();
                    }
                    if let Some(name) = &c.display_name {
                        s.display_name = name.clone();
                    }
                })
                .or_insert_with(|| ServerState {
                    display_name: c.display_name.clone().unwrap_or_else(|| c.id.clone()),
                    description: c.description.clone().unwrap_or_default(),
                    category: c.category.clone().unwrap_or_else(|| "Custom".into()),
                    command: c.command.clone(),
                    args: c.args.clone(),
                    secret_specs,
                    enabled: c.enabled,
                    status: ServerStatus::Disconnected,
                    last_error: None,
                    client: None,
                    registered_tool_names: Vec::new(),
                    server_version: String::new(),
                });
        }
    }

    /// Spawn every enabled server (concurrently) and register their tools.
    pub async fn connect_all_enabled(&self) {
        // Snapshot list of ids first to avoid holding the write-lock across awaits.
        let ids: Vec<String> = {
            let map = self.servers.read().await;
            map.iter()
                .filter(|(_, s)| s.enabled)
                .map(|(id, _)| id.clone())
                .collect()
        };
        for id in ids {
            let _ = self.connect(&id).await;
        }
    }

    pub async fn connect(&self, id: &str) -> Result<usize, String> {
        // Mark connecting.
        let (command, args, secret_specs) = {
            let mut map = self.servers.write().await;
            let s = map.get_mut(id).ok_or_else(|| format!("unknown MCP server: {id}"))?;
            if s.command.is_empty() {
                let msg = "no command configured".to_string();
                s.status = ServerStatus::Error(msg.clone());
                s.last_error = Some(msg.clone());
                return Err(msg);
            }
            s.status = ServerStatus::Connecting;
            s.last_error = None;
            (s.command.clone(), s.args.clone(), s.secret_specs.clone())
        };

        // Build env from secrets in keystore (or env vars).
        let mut env: HashMap<String, String> = HashMap::new();
        let mut missing_required: Vec<String> = Vec::new();
        {
            let ks = self.keystore.lock().await;
            for spec in &secret_specs {
                let stored_key = mcp_secret_key(id, &spec.key);
                let val = std::env::var(&spec.key)
                    .ok()
                    .filter(|v| !v.is_empty())
                    .or_else(|| ks.get(&stored_key));
                match val {
                    Some(v) => {
                        env.insert(spec.key.clone(), v);
                    }
                    None if spec.required => {
                        missing_required.push(spec.key.clone());
                    }
                    None => {}
                }
            }
        }

        // For the filesystem server, the allowed root is passed as a CLI
        // arg (not env). Append it after the package args.
        let mut effective_args = args.clone();
        if id == "filesystem" {
            if let Some(root) = env.remove("MCP_FS_ROOT") {
                effective_args.push(root);
            }
        }

        if !missing_required.is_empty() {
            let msg = format!(
                "missing required secrets: {}",
                missing_required.join(", ")
            );
            let mut map = self.servers.write().await;
            if let Some(s) = map.get_mut(id) {
                s.status = ServerStatus::Error(msg.clone());
                s.last_error = Some(msg.clone());
            }
            return Err(msg);
        }

        // Spawn + handshake.
        let res = McpClient::connect_stdio(
            &command,
            &effective_args,
            &env,
            None,
            self.connect_timeout,
        )
        .await;
        let client = match res {
            Ok(c) => Arc::new(c),
            Err(e) => {
                let msg = format!("{e}");
                let mut map = self.servers.write().await;
                if let Some(s) = map.get_mut(id) {
                    s.status = ServerStatus::Error(msg.clone());
                    s.last_error = Some(msg.clone());
                }
                return Err(msg);
            }
        };

        // List tools.
        let tools = match client.list_tools().await {
            Ok(t) => t,
            Err(e) => {
                let msg = format!("tools/list failed: {e}");
                client.shutdown().await;
                let mut map = self.servers.write().await;
                if let Some(s) = map.get_mut(id) {
                    s.status = ServerStatus::Error(msg.clone());
                    s.last_error = Some(msg.clone());
                }
                return Err(msg);
            }
        };

        let perm = default_permission_for(id);
        let mut local_names: Vec<String> = Vec::with_capacity(tools.len());
        for t in &tools {
            let local_name = format!("mcp__{id}__{}", sanitize_name(&t.name));
            let schema = t.input_schema.clone().unwrap_or_else(|| {
                serde_json::json!({ "type": "object", "properties": {} })
            });
            // Prepend a generic authentication-context tag so the model
            // sees, on EVERY MCP tool from EVERY server, that this call
            // runs as the user's authenticated identity. This nudges the
            // model away from asking the user for usernames/owners that
            // the server already knows from the credential, and away from
            // calling generic search tools with placeholder strings.
            // The tag is server-agnostic — the same wording is applied
            // to all 50+ catalog entries and any user-defined custom
            // server, because the principle is identical for all of them.
            let description_text = format!(
                "[mcp:{}] (authenticated as the user's own account on this \
                 service — do NOT pass placeholder values like USERNAME / \
                 OWNER / ME / YOUR_TOKEN; the server already knows the \
                 user's identity from its credential.) {}",
                id,
                t.description.clone().unwrap_or_else(|| t.name.clone())
            );
            self.registry.register(Box::new(McpTool {
                local_name: local_name.clone(),
                server_id: id.to_string(),
                remote_name: t.name.clone(),
                description_text,
                schema,
                default_permission: perm.clone(),
                client: client.clone(),
            }));
            local_names.push(local_name);
        }

        let server_version = client.handshake.server_version.clone();
        let tool_count = local_names.len();
        {
            let mut map = self.servers.write().await;
            if let Some(s) = map.get_mut(id) {
                s.status = ServerStatus::Active;
                s.client = Some(client);
                s.registered_tool_names = local_names;
                s.server_version = server_version;
            }
        }
        Ok(tool_count)
    }

    pub async fn disconnect(&self, id: &str) -> Result<(), String> {
        let (client_opt, names) = {
            let mut map = self.servers.write().await;
            let s = map.get_mut(id).ok_or_else(|| format!("unknown server: {id}"))?;
            let c = s.client.take();
            let n = std::mem::take(&mut s.registered_tool_names);
            s.status = ServerStatus::Disconnected;
            (c, n)
        };
        for n in &names {
            self.registry.unregister(n);
        }
        if let Some(c) = client_opt {
            c.shutdown().await;
        }
        Ok(())
    }

    pub async fn set_enabled(
        &self,
        id: &str,
        enabled: bool,
    ) -> Result<bool, String> {
        let was_enabled = {
            let mut map = self.servers.write().await;
            let s = map
                .get_mut(id)
                .ok_or_else(|| format!("unknown server: {id}"))?;
            let prev = s.enabled;
            s.enabled = enabled;
            prev
        };
        if !enabled && was_enabled {
            self.disconnect(id).await.ok();
        }
        Ok(was_enabled != enabled)
    }

    /// Add a brand-new server (not in the catalog) at runtime. Persisted
    /// caller-side via `export_to_config`. Returns Err on validation
    /// failures or id-collision.
    pub async fn add_custom_server(
        &self,
        id: &str,
        display_name: &str,
        description: &str,
        category: &str,
        command: &str,
        args: Vec<String>,
        secret_keys: Vec<String>,
        enabled: bool,
    ) -> Result<(), String> {
        let id = id.trim();
        if id.is_empty() {
            return Err("id cannot be empty".into());
        }
        if !id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') {
            return Err("id may only contain a–z, 0–9, '-', '_'".into());
        }
        if command.trim().is_empty() {
            return Err("command cannot be empty".into());
        }
        if catalog::lookup(id).is_some() {
            return Err(format!(
                "'{id}' is a built-in catalog entry — pick a different id"
            ));
        }
        {
            let map = self.servers.read().await;
            if map.contains_key(id) {
                return Err(format!("server id '{id}' already exists"));
            }
        }
        let secret_specs: Vec<catalog::SecretSpec> = secret_keys
            .into_iter()
            .map(|k| {
                let key = k.trim().to_string();
                catalog::SecretSpec {
                    label: key.clone(),
                    help: format!("Custom secret — set as env var '{key}' for the server"),
                    key,
                    required: true,
                }
            })
            .filter(|s| !s.key.is_empty())
            .collect();
        let state = ServerState {
            display_name: if display_name.trim().is_empty() {
                id.to_string()
            } else {
                display_name.trim().to_string()
            },
            description: description.trim().to_string(),
            category: if category.trim().is_empty() {
                "Custom".to_string()
            } else {
                category.trim().to_string()
            },
            command: command.trim().to_string(),
            args,
            secret_specs,
            enabled,
            status: ServerStatus::Disconnected,
            last_error: None,
            client: None,
            registered_tool_names: Vec::new(),
            server_version: String::new(),
        };
        let mut map = self.servers.write().await;
        map.insert(id.to_string(), state);
        Ok(())
    }

    /// Permanently remove a server entry from the manager. Catalog entries
    /// cannot be deleted (they always reload at startup); only custom
    /// servers added via `add_custom_server` are removed.
    pub async fn remove_custom_server(&self, id: &str) -> Result<(), String> {
        if catalog::lookup(id).is_some() {
            return Err(format!("'{id}' is a built-in entry; cannot delete"));
        }
        // Disconnect first so we drop the child cleanly.
        let _ = self.disconnect(id).await;
        let mut map = self.servers.write().await;
        map.remove(id)
            .map(|_| ())
            .ok_or_else(|| format!("unknown server: {id}"))
    }

    pub async fn save_secret(
        &self,
        id: &str,
        key: &str,
        value: &str,
    ) -> Result<(), String> {
        let storage_key = mcp_secret_key(id, key);
        let mut ks = self.keystore.lock().await;
        ks.set(&storage_key, value).map_err(|e| e.to_string())
    }

    pub async fn delete_secret(&self, id: &str, key: &str) -> Result<(), String> {
        let storage_key = mcp_secret_key(id, key);
        let mut ks = self.keystore.lock().await;
        ks.delete(&storage_key).map_err(|e| e.to_string())
    }

    /// Build a snapshot list for the TUI.
    pub async fn snapshot(&self) -> Vec<ServerSnapshot> {
        let map = self.servers.read().await;
        let mut out = Vec::with_capacity(map.len());
        let ks = self.keystore.lock().await;
        for (id, s) in map.iter() {
            let mut secrets = Vec::new();
            for spec in &s.secret_specs {
                let env_present = std::env::var(&spec.key)
                    .ok()
                    .map(|v| !v.is_empty())
                    .unwrap_or(false);
                let stored_present = ks
                    .get(&mcp_secret_key(id, &spec.key))
                    .map(|v| !v.is_empty())
                    .unwrap_or(false);
                let source = if stored_present {
                    SecretSource::Stored
                } else if env_present {
                    SecretSource::Env
                } else {
                    SecretSource::None
                };
                secrets.push(SecretStatus {
                    key: spec.key.clone(),
                    label: spec.label.clone(),
                    help: spec.help.clone(),
                    required: spec.required,
                    present: source != SecretSource::None,
                    source,
                });
            }
            let mut recent = Vec::new();
            if let Some(c) = &s.client {
                recent = c.transport.stderr_snapshot().await;
                if recent.len() > 5 {
                    let from = recent.len() - 5;
                    recent = recent[from..].to_vec();
                }
            }
            out.push(ServerSnapshot {
                id: id.clone(),
                display_name: s.display_name.clone(),
                description: s.description.clone(),
                category: s.category.clone(),
                enabled: s.enabled,
                status: if !s.enabled {
                    ServerStatus::Disabled
                } else {
                    s.status.clone()
                },
                tool_count: s.registered_tool_names.len(),
                server_version: s.server_version.clone(),
                recent_stderr: recent,
                required_secrets: secrets,
                last_error: s.last_error.clone(),
            });
        }
        out.sort_by(|a, b| a.display_name.to_lowercase().cmp(&b.display_name.to_lowercase()));
        out
    }

    /// Persist all current server enabled-states + commands to config.
    pub async fn export_to_config(&self) -> Vec<McpServerConfig> {
        let map = self.servers.read().await;
        let mut out: Vec<McpServerConfig> = map
            .iter()
            .filter(|(id, s)| {
                // Only persist if user has done something with it: enabled,
                // or non-catalog custom entry.
                s.enabled || catalog::lookup(id).is_none()
            })
            .map(|(id, s)| {
                // For catalog entries, leave command empty so we always pick
                // up upstream package updates; only persist enabled flag.
                let is_catalog = catalog::lookup(id).is_some();
                McpServerConfig {
                    id: id.clone(),
                    display_name: if is_catalog {
                        None
                    } else {
                        Some(s.display_name.clone())
                    },
                    description: if is_catalog {
                        None
                    } else {
                        Some(s.description.clone())
                    },
                    category: if is_catalog {
                        None
                    } else {
                        Some(s.category.clone())
                    },
                    command: if is_catalog { String::new() } else { s.command.clone() },
                    args: if is_catalog { Vec::new() } else { s.args.clone() },
                    secret_specs: if is_catalog {
                        Vec::new()
                    } else {
                        s.secret_specs.clone()
                    },
                    enabled: s.enabled,
                }
            })
            .collect();
        out.sort_by(|a, b| a.id.cmp(&b.id));
        out
    }
}

fn state_from_catalog(entry: &CatalogEntry, enabled: bool) -> ServerState {
    ServerState {
        display_name: entry.display_name.to_string(),
        description: entry.description.to_string(),
        category: entry.category.to_string(),
        command: entry.command.to_string(),
        args: entry.args.iter().map(|s| s.to_string()).collect(),
        secret_specs: entry.secret_specs(),
        enabled,
        status: ServerStatus::Disconnected,
        last_error: None,
        client: None,
        registered_tool_names: Vec::new(),
        server_version: String::new(),
    }
}

fn default_permission_for(_id: &str) -> PermissionLevel {
    // We err on the side of safety: every MCP tool is treated as Mutating
    // unless the user grants always-allow. This routes through the existing
    // permission prompt for non-trust-mode sessions.
    PermissionLevel::Mutating
}

fn sanitize_name(name: &str) -> String {
    name.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

pub fn mcp_secret_key(server_id: &str, key: &str) -> String {
    format!("mcp:{server_id}:{key}")
}
