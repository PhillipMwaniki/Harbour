//! A Model Context Protocol server over Harbour's core.
//!
//! It speaks MCP (JSON-RPC 2.0) so an AI agent can work with the same saved
//! hosts the desktop app uses. The protocol dispatch and every tool live here,
//! behind a [`Server`] that owns the vault; `main` wires it to stdin/stdout.
//!
//! The core is reused directly - [`Vault`] and, in later phases, the SSH client
//! and secret store - with no Tauri in the build. Everything an agent can do is
//! bounded by what the person who launched the server already has: their saved
//! hosts, their keychain, their `known_hosts`.

use std::path::PathBuf;
use std::sync::Arc;

use serde::Deserialize;
use serde_json::{json, Value};

use harbour_lib::vault::store::Vault;

/// The MCP protocol version this server implements.
const PROTOCOL_VERSION: &str = "2024-11-05";

/// Where Harbour keeps its config, so the server reads the same vault the app
/// wrote. `HARBOUR_CONFIG_DIR` overrides it - the way to point at a portable
/// install, whose files sit beside the app rather than in the OS config dir.
pub fn config_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("HARBOUR_CONFIG_DIR") {
        return PathBuf::from(dir);
    }
    dirs::config_dir()
        .map(|dir| dir.join("com.harbour.app"))
        .unwrap_or_else(|| std::env::temp_dir().join("harbour"))
}

/// Opens the vault at the shared config location, or an empty in-memory one if
/// it will not open - the same fallback the app makes, so the server still
/// starts and simply reports no hosts.
pub fn open_vault() -> Arc<Vault> {
    let path = config_dir().join("vault.sqlite3");
    let vault = Vault::open(&path).unwrap_or_else(|err| {
        tracing::error!(error = %err, path = %path.display(), "could not open the vault; starting empty");
        Vault::in_memory().expect("an in-memory vault must always open")
    });
    Arc::new(vault)
}

/// One incoming JSON-RPC message. A request carries an `id`; a notification
/// does not, and gets no reply.
#[derive(Debug, Deserialize)]
pub struct Incoming {
    pub id: Option<Value>,
    pub method: String,
    #[serde(default)]
    pub params: Value,
}

/// The MCP server: the protocol, and the tools, over one vault.
pub struct Server {
    vault: Arc<Vault>,
}

impl Server {
    pub fn new(vault: Arc<Vault>) -> Self {
        Self { vault }
    }

    /// Handles one message, returning the reply to write back - or `None` for a
    /// notification, which the protocol says is never answered.
    pub async fn handle(&self, incoming: Incoming) -> Option<Value> {
        let id = incoming.id.clone();
        match incoming.method.as_str() {
            "initialize" => id.map(|id| success(id, self.initialize())),
            "tools/list" => id.map(|id| success(id, self.tools_list())),
            "tools/call" => {
                // A call always has an id; a "call" without one is malformed and
                // ignored rather than answered into the void.
                let id = id?;
                Some(success(id, self.tools_call(incoming.params).await))
            }
            // Liveness check some clients send.
            "ping" => id.map(|id| success(id, json!({}))),
            // Notifications (initialized, cancelled, …) are acknowledged by
            // silence.
            method if method.starts_with("notifications/") => None,
            // Anything else: method-not-found, but only if a reply is expected.
            _ => id.map(|id| error(id, -32601, "method not found")),
        }
    }

    fn initialize(&self) -> Value {
        json!({
            "protocolVersion": PROTOCOL_VERSION,
            "capabilities": { "tools": {} },
            "serverInfo": { "name": "harbour", "version": env!("CARGO_PKG_VERSION") },
        })
    }

    fn tools_list(&self) -> Value {
        json!({ "tools": tool_specs() })
    }

    /// Runs the named tool. A tool's own failure is reported inside the result
    /// as `isError`, not as a JSON-RPC error, which is how MCP wants a failed
    /// tool distinguished from a broken request.
    async fn tools_call(&self, params: Value) -> Value {
        let name = params.get("name").and_then(Value::as_str).unwrap_or("");
        let arguments = params.get("arguments").cloned().unwrap_or(Value::Null);

        let outcome = match name {
            "harbour_list_hosts" => self.list_hosts().await,
            "harbour_list_folders" => self.list_folders().await,
            other => Err(format!("unknown tool `{other}`")),
        };
        let _ = arguments; // no tool takes arguments yet

        match outcome {
            Ok(value) => tool_text(&value, false),
            Err(message) => tool_text(&json!({ "error": message }), true),
        }
    }

    async fn list_hosts(&self) -> Result<Value, String> {
        let tree = self.read_tree().await?;
        let hosts: Vec<Value> = tree
            .hosts
            .iter()
            .map(|host| {
                json!({
                    "id": host.id,
                    "name": host.name,
                    "hostname": host.hostname,
                    "port": host.port,
                    "username": host.username,
                    "folderId": host.folder_id,
                    "jumpHostId": host.jump_host_id,
                    "guarded": host.guarded,
                    "hasSavedPassword": host.has_saved_password,
                })
            })
            .collect();
        Ok(json!({ "hosts": hosts }))
    }

    async fn list_folders(&self) -> Result<Value, String> {
        let tree = self.read_tree().await?;
        let folders: Vec<Value> = tree
            .folders
            .iter()
            .map(|folder| {
                json!({
                    "id": folder.id,
                    "name": folder.name,
                    "parentId": folder.parent_id,
                })
            })
            .collect();
        Ok(json!({ "folders": folders }))
    }

    /// Reads the vault tree off the async runtime: the vault is synchronous
    /// SQLite, so it must not run on a reactor thread.
    async fn read_tree(&self) -> Result<harbour_lib::vault::model::VaultTree, String> {
        let vault = Arc::clone(&self.vault);
        tokio::task::spawn_blocking(move || vault.tree())
            .await
            .map_err(|err| format!("vault read task failed: {err}"))?
            .map_err(|err| err.to_string())
    }
}

/// The tools this server advertises, with their JSON schemas.
fn tool_specs() -> Vec<Value> {
    vec![
        json!({
            "name": "harbour_list_hosts",
            "description": "List the SSH hosts saved in Harbour's vault, with where to connect and as whom. Read-only.",
            "inputSchema": { "type": "object", "properties": {}, "additionalProperties": false },
        }),
        json!({
            "name": "harbour_list_folders",
            "description": "List the folders in Harbour's vault, for grouping saved hosts. Read-only.",
            "inputSchema": { "type": "object", "properties": {}, "additionalProperties": false },
        }),
    ]
}

/// A successful JSON-RPC reply.
fn success(id: Value, result: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "result": result })
}

/// A JSON-RPC error reply - for a broken request, not a failed tool.
fn error(id: Value, code: i64, message: &str) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })
}

/// A tool result: the value as pretty JSON text, flagged as an error or not.
fn tool_text(value: &Value, is_error: bool) -> Value {
    let text = serde_json::to_string_pretty(value).unwrap_or_else(|_| value.to_string());
    json!({
        "content": [{ "type": "text", "text": text }],
        "isError": is_error,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use harbour_lib::vault::model::{HostAuth, HostInput};

    fn incoming(method: &str, id: Option<Value>, params: Value) -> Incoming {
        Incoming {
            id,
            method: method.to_string(),
            params,
        }
    }

    fn server_with_a_host() -> Server {
        let vault = Vault::in_memory().unwrap();
        vault
            .create_host(HostInput {
                folder_id: None,
                name: "web".into(),
                hostname: "web.example.com".into(),
                port: 22,
                username: "deploy".into(),
                description: None,
                auth: HostAuth {
                    use_agent: true,
                    key_path: None,
                    use_password: true,
                },
                jump_host_id: None,
                guarded: false,
            })
            .unwrap();
        Server::new(Arc::new(vault))
    }

    #[tokio::test]
    async fn initialize_reports_tool_capability_and_version() {
        let server = Server::new(Arc::new(Vault::in_memory().unwrap()));
        let reply = server
            .handle(incoming("initialize", Some(json!(1)), Value::Null))
            .await
            .expect("initialize is answered");

        assert_eq!(reply["result"]["protocolVersion"], PROTOCOL_VERSION);
        assert!(reply["result"]["capabilities"]["tools"].is_object());
        assert_eq!(reply["result"]["serverInfo"]["name"], "harbour");
    }

    #[tokio::test]
    async fn a_notification_is_not_answered() {
        let server = Server::new(Arc::new(Vault::in_memory().unwrap()));
        let reply = server
            .handle(incoming("notifications/initialized", None, Value::Null))
            .await;
        assert!(reply.is_none());
    }

    #[tokio::test]
    async fn tools_list_advertises_the_inventory_tools() {
        let server = Server::new(Arc::new(Vault::in_memory().unwrap()));
        let reply = server
            .handle(incoming("tools/list", Some(json!(2)), Value::Null))
            .await
            .unwrap();

        let names: Vec<&str> = reply["result"]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .map(|tool| tool["name"].as_str().unwrap())
            .collect();
        assert!(names.contains(&"harbour_list_hosts"));
        assert!(names.contains(&"harbour_list_folders"));
    }

    #[tokio::test]
    async fn list_hosts_returns_the_saved_hosts() {
        let server = server_with_a_host();
        let reply = server
            .handle(incoming(
                "tools/call",
                Some(json!(3)),
                json!({ "name": "harbour_list_hosts", "arguments": {} }),
            ))
            .await
            .unwrap();

        assert_eq!(reply["result"]["isError"], false);
        let text = reply["result"]["content"][0]["text"].as_str().unwrap();
        let parsed: Value = serde_json::from_str(text).unwrap();
        assert_eq!(parsed["hosts"][0]["name"], "web");
        assert_eq!(parsed["hosts"][0]["hostname"], "web.example.com");
        assert_eq!(parsed["hosts"][0]["username"], "deploy");
    }

    #[tokio::test]
    async fn an_unknown_tool_is_a_tool_error_not_a_protocol_error() {
        let server = Server::new(Arc::new(Vault::in_memory().unwrap()));
        let reply = server
            .handle(incoming(
                "tools/call",
                Some(json!(4)),
                json!({ "name": "harbour_nope", "arguments": {} }),
            ))
            .await
            .unwrap();

        // The request succeeded; the tool reported the failure.
        assert!(reply.get("error").is_none());
        assert_eq!(reply["result"]["isError"], true);
    }

    #[tokio::test]
    async fn an_unknown_method_is_method_not_found() {
        let server = Server::new(Arc::new(Vault::in_memory().unwrap()));
        let reply = server
            .handle(incoming("frobnicate", Some(json!(5)), Value::Null))
            .await
            .unwrap();
        assert_eq!(reply["error"]["code"], -32601);
    }
}
