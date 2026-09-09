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

use harbour_lib::error::AppResult;
use harbour_lib::ssh::client::{self, Endpoint};
use harbour_lib::ssh::known_hosts::KnownHosts;
use harbour_lib::ssh::{
    Asker, HostKeyAnswer, HostKeyQuestion, SecretAnswer, SecretKind, SecretQuestion,
};
use harbour_lib::vault::model::{Host, HostId};
use harbour_lib::vault::secrets::{SecretSlot, SecretStore};
use harbour_lib::vault::store::Vault;

/// How many hosts a fleet run connects to at once - the fleet runner's bound.
const FLEET_CONCURRENCY: usize = 8;

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

/// The shared state the server reads from the config directory: the vault, the
/// host-key store, and the secret store. Opened once at startup.
pub struct Core {
    pub vault: Arc<Vault>,
    pub known_hosts: Arc<KnownHosts>,
    pub secrets: Arc<SecretStore>,
}

/// Opens the vault, `known_hosts` and secret store at the shared config
/// location - the same files, in the same way, as the desktop app. A vault
/// that will not open falls back to an empty in-memory one, so the server still
/// starts and simply reports no hosts.
pub fn open_core() -> Core {
    let dir = config_dir();
    let vault_path = dir.join("vault.sqlite3");
    let vault = Vault::open(&vault_path).unwrap_or_else(|err| {
        tracing::error!(error = %err, path = %vault_path.display(), "could not open the vault; starting empty");
        Vault::in_memory().expect("an in-memory vault must always open")
    });
    Core {
        vault: Arc::new(vault),
        known_hosts: Arc::new(KnownHosts::new(dir.join("known_hosts"))),
        secrets: Arc::new(SecretStore::detect(dir.join("secrets.vault"))),
    }
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

/// The MCP server: the protocol, and the tools, over the shared core.
pub struct Server {
    vault: Arc<Vault>,
    known_hosts: Arc<KnownHosts>,
    secrets: Arc<SecretStore>,
    /// Whether tools that act - run commands, and later write files and open
    /// forwards - are offered. Off by default: an agent can see the estate but
    /// not touch it until the server is started with `--allow-write`.
    allow_write: bool,
}

impl Server {
    /// A read-only server over just a vault. Used by the inventory tests and
    /// the read-only default when nothing else is wired. The secret store is
    /// file-backed rather than detected, so building one never probes the OS
    /// keychain - inventory reads no secrets anyway.
    pub fn new(vault: Arc<Vault>) -> Self {
        Self {
            vault,
            known_hosts: Arc::new(KnownHosts::new(config_dir().join("known_hosts"))),
            secrets: Arc::new(SecretStore::file_backed(config_dir().join("secrets.vault"))),
            allow_write: false,
        }
    }

    /// A server over the full core, with execution enabled or not.
    pub fn with_core(core: Core, allow_write: bool) -> Self {
        Self {
            vault: core.vault,
            known_hosts: core.known_hosts,
            secrets: core.secrets,
            allow_write,
        }
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
        json!({ "tools": tool_specs(self.allow_write) })
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
            "harbour_run_command" => self.run_command(arguments).await,
            "harbour_run_fleet" => self.run_fleet(arguments).await,
            other => Err(format!("unknown tool `{other}`")),
        };

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

    async fn run_command(&self, args: Value) -> Result<Value, String> {
        if !self.allow_write {
            return Err(WRITE_DISABLED.into());
        }
        let host = args
            .get("host")
            .and_then(Value::as_str)
            .ok_or("`host` is required (a host id or name)")?;
        let command = args
            .get("command")
            .and_then(Value::as_str)
            .ok_or("`command` is required")?;
        if command.trim().is_empty() {
            return Err("`command` is empty".into());
        }
        // A single run reports a failure to reach or run as a tool error; a
        // command that ran and exited non-zero is a success carrying that code.
        let result = self.exec(host, command).await;
        match result.error {
            Some(error) => Err(error),
            None => Ok(result.into_value()),
        }
    }

    async fn run_fleet(&self, args: Value) -> Result<Value, String> {
        if !self.allow_write {
            return Err(WRITE_DISABLED.into());
        }
        let hosts: Vec<String> = args
            .get("hosts")
            .and_then(Value::as_array)
            .ok_or("`hosts` is required (an array of host ids or names)")?
            .iter()
            .filter_map(|value| value.as_str().map(str::to_string))
            .collect();
        let command = args
            .get("command")
            .and_then(Value::as_str)
            .ok_or("`command` is required")?
            .to_string();
        if command.trim().is_empty() {
            return Err("`command` is empty".into());
        }
        if hosts.is_empty() {
            return Ok(json!({ "results": [] }));
        }

        let semaphore = Arc::new(tokio::sync::Semaphore::new(FLEET_CONCURRENCY));
        let mut tasks = tokio::task::JoinSet::new();
        for host in hosts {
            let server = self.clone_refs();
            let command = command.clone();
            let semaphore = Arc::clone(&semaphore);
            tasks.spawn(async move {
                let _permit = semaphore.acquire().await;
                server.exec(&host, &command).await
            });
        }

        let mut results = Vec::new();
        while let Some(joined) = tasks.join_next().await {
            if let Ok(result) = joined {
                results.push(result.into_value());
            }
        }
        Ok(json!({ "results": results }))
    }

    /// Resolves a host, checks it, and runs one command on it - the shared body
    /// of the single-host and fleet tools. Every failure is captured in the
    /// result rather than thrown, so one bad host does not sink a fleet run.
    async fn exec(&self, spec: &str, command: &str) -> ExecResult {
        let host = match self.resolve_host(spec).await {
            Ok(host) => host,
            Err(err) => return ExecResult::failed(spec, err),
        };
        // A guarded host expects a person to confirm destructive commands; there
        // is no one to ask over MCP, so it is refused rather than run blind.
        if host.guarded {
            return ExecResult::failed(
                &host.name,
                format!(
                    "`{}` is guarded; run commands on it from the Harbour app, where they can be confirmed",
                    host.name
                ),
            );
        }

        let chain = match self.resolve_chain(&host.id).await {
            Ok(chain) => chain,
            Err(err) => return ExecResult::failed(&host.name, err),
        };
        for hop in &chain {
            if hop.auth.methods().is_empty() {
                return ExecResult::failed(
                    &host.name,
                    format!("{} has no authentication method enabled", hop.name),
                );
            }
        }

        let endpoint = |host: &Host| Endpoint {
            target: host.target(),
            methods: host.auth.methods(),
            asker: Arc::new(McpAsker {
                host_id: host.id.clone(),
                secrets: Arc::clone(&self.secrets),
            }),
        };
        let dest = endpoint(&chain[0]);
        let jumps: Vec<Endpoint> = chain[1..].iter().rev().map(endpoint).collect();

        match client::run_command(jumps, dest, Arc::clone(&self.known_hosts), command).await {
            Ok(outcome) => ExecResult {
                name: host.name,
                exit_code: outcome.exit_code,
                stdout: String::from_utf8_lossy(&outcome.stdout).into_owned(),
                stderr: String::from_utf8_lossy(&outcome.stderr).into_owned(),
                error: None,
            },
            Err(err) => ExecResult::failed(&host.name, err.to_string()),
        }
    }

    /// A host by id, or by name when the name is unique. Names are what a person
    /// calls a host, so an agent naming one is the friendly path; an id is the
    /// unambiguous one.
    async fn resolve_host(&self, spec: &str) -> Result<Host, String> {
        let vault = Arc::clone(&self.vault);
        let spec = spec.to_string();
        tokio::task::spawn_blocking(move || {
            if let Ok(host) = vault.host(&spec) {
                return Ok(host);
            }
            let tree = vault.tree().map_err(|err| err.to_string())?;
            let mut named: Vec<Host> = tree.hosts.into_iter().filter(|h| h.name == spec).collect();
            match named.len() {
                0 => Err(format!("no host with id or name `{spec}`")),
                1 => Ok(named.remove(0)),
                _ => Err(format!("`{spec}` matches more than one host; use its id")),
            }
        })
        .await
        .map_err(|err| format!("host lookup task failed: {err}"))?
    }

    async fn resolve_chain(&self, host_id: &str) -> Result<Vec<Host>, String> {
        let vault = Arc::clone(&self.vault);
        let id = host_id.to_string();
        tokio::task::spawn_blocking(move || vault.resolve_chain(&id))
            .await
            .map_err(|err| format!("chain lookup task failed: {err}"))?
            .map_err(|err| err.to_string())
    }

    /// A cheap clone of just the references a spawned fleet task needs.
    fn clone_refs(&self) -> Self {
        Self {
            vault: Arc::clone(&self.vault),
            known_hosts: Arc::clone(&self.known_hosts),
            secrets: Arc::clone(&self.secrets),
            allow_write: self.allow_write,
        }
    }
}

/// The message a write tool gives when the server was not started to allow it.
const WRITE_DISABLED: &str =
    "command execution is disabled; start the server with --allow-write (or HARBOUR_MCP_ALLOW_WRITE=1)";

/// What running a command on one host came to - the same shape the fleet runner
/// reports, so a single run and a fleet run read alike.
struct ExecResult {
    name: String,
    exit_code: Option<u32>,
    stdout: String,
    stderr: String,
    error: Option<String>,
}

impl ExecResult {
    fn failed(name: &str, error: String) -> Self {
        Self {
            name: name.to_string(),
            exit_code: None,
            stdout: String::new(),
            stderr: String::new(),
            error: Some(error),
        }
    }

    fn into_value(self) -> Value {
        json!({
            "host": self.name,
            "exitCode": self.exit_code,
            "stdout": self.stdout,
            "stderr": self.stderr,
            "error": self.error,
        })
    }
}

/// Answers a connection's prompts without ever asking a person: a saved secret
/// if the keychain holds one, and nothing otherwise. A host key that is not
/// already trusted is refused - trusting a new one is a decision for a person at
/// the app, not for an unattended agent. This is the fleet runner's asker.
struct McpAsker {
    host_id: HostId,
    secrets: Arc<SecretStore>,
}

impl Asker for McpAsker {
    async fn host_key(&self, _question: HostKeyQuestion) -> AppResult<HostKeyAnswer> {
        Ok(HostKeyAnswer {
            accept: false,
            remember: false,
        })
    }

    async fn secret(&self, question: SecretQuestion) -> AppResult<SecretAnswer> {
        let slot = match question.kind {
            SecretKind::Password => Some(SecretSlot::Password),
            SecretKind::Passphrase => Some(SecretSlot::KeyPassphrase),
            SecretKind::Challenge => None,
        };
        if let Some(slot) = slot {
            let host = self.host_id.clone();
            let secrets = Arc::clone(&self.secrets);
            let read = tokio::task::spawn_blocking(move || secrets.get(&host, slot)).await;
            if let Ok(Ok(Some(secret))) = read {
                return Ok(SecretAnswer {
                    secret: Some(secret),
                    remember: false,
                });
            }
        }
        Ok(SecretAnswer {
            secret: None,
            remember: false,
        })
    }
}

/// The tools this server advertises, with their JSON schemas. The execution
/// tools appear only when the server was started to allow them, so an agent
/// sees exactly what it may do.
fn tool_specs(allow_write: bool) -> Vec<Value> {
    let mut tools = vec![
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
    ];
    if allow_write {
        tools.push(json!({
            "name": "harbour_run_command",
            "description": "Run a shell command on a saved host over SSH and return its stdout, stderr and exit code. Non-interactive: uses the keychain password if there is one and refuses hosts whose key is not already trusted. Guarded hosts are refused.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "host": { "type": "string", "description": "A saved host's id, or its unique name." },
                    "command": { "type": "string" },
                },
                "required": ["host", "command"],
                "additionalProperties": false,
            },
        }));
        tools.push(json!({
            "name": "harbour_run_fleet",
            "description": "Run one command on several saved hosts at once, returning a result per host. Same rules as harbour_run_command.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "hosts": { "type": "array", "items": { "type": "string" }, "description": "Saved host ids or unique names." },
                    "command": { "type": "string" },
                },
                "required": ["hosts", "command"],
                "additionalProperties": false,
            },
        }));
    }
    tools
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

    fn host_input(name: &str) -> HostInput {
        HostInput {
            folder_id: None,
            name: name.into(),
            hostname: format!("{name}.example.com"),
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
        }
    }

    fn server_with_a_host() -> Server {
        let vault = Vault::in_memory().unwrap();
        vault.create_host(host_input("web")).unwrap();
        Server::new(Arc::new(vault))
    }

    /// A write-enabled server over `vault`, with a file-backed secret store in a
    /// temp path so nothing touches the real keychain or config.
    fn writable_server(vault: Vault) -> Server {
        let dir = std::env::temp_dir();
        Server::with_core(
            Core {
                vault: Arc::new(vault),
                known_hosts: Arc::new(KnownHosts::new(dir.join("harbour-mcp-test-known_hosts"))),
                secrets: Arc::new(SecretStore::file_backed(
                    dir.join("harbour-mcp-test-secrets"),
                )),
            },
            true,
        )
    }

    async fn call(server: &Server, name: &str, arguments: Value) -> Value {
        server
            .handle(incoming(
                "tools/call",
                Some(json!(1)),
                json!({ "name": name, "arguments": arguments }),
            ))
            .await
            .unwrap()
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

    #[tokio::test]
    async fn execution_tools_are_hidden_and_refused_without_allow_write() {
        let server = server_with_a_host(); // read-only
        let list = server
            .handle(incoming("tools/list", Some(json!(1)), Value::Null))
            .await
            .unwrap();
        let names: Vec<String> = list["result"]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .map(|tool| tool["name"].as_str().unwrap().to_string())
            .collect();
        assert!(!names.iter().any(|n| n == "harbour_run_command"));

        let reply = call(
            &server,
            "harbour_run_command",
            json!({ "host": "web", "command": "id" }),
        )
        .await;
        assert_eq!(reply["result"]["isError"], true);
        let text = reply["result"]["content"][0]["text"].as_str().unwrap();
        assert!(text.contains("--allow-write"), "got {text}");
    }

    #[tokio::test]
    async fn execution_tools_appear_with_allow_write() {
        let server = writable_server(Vault::in_memory().unwrap());
        let list = server
            .handle(incoming("tools/list", Some(json!(1)), Value::Null))
            .await
            .unwrap();
        let names: Vec<String> = list["result"]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .map(|tool| tool["name"].as_str().unwrap().to_string())
            .collect();
        assert!(names.iter().any(|n| n == "harbour_run_command"));
        assert!(names.iter().any(|n| n == "harbour_run_fleet"));
    }

    #[tokio::test]
    async fn a_guarded_host_is_refused() {
        let vault = Vault::in_memory().unwrap();
        vault
            .create_host(HostInput {
                guarded: true,
                ..host_input("prod")
            })
            .unwrap();
        let server = writable_server(vault);

        let reply = call(
            &server,
            "harbour_run_command",
            json!({ "host": "prod", "command": "rm -rf /" }),
        )
        .await;

        assert_eq!(reply["result"]["isError"], true);
        let parsed: Value =
            serde_json::from_str(reply["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
        assert!(parsed["error"].as_str().unwrap().contains("guarded"));
    }

    #[tokio::test]
    async fn an_unknown_host_is_reported() {
        let server = writable_server(Vault::in_memory().unwrap());
        let reply = call(
            &server,
            "harbour_run_command",
            json!({ "host": "ghost", "command": "id" }),
        )
        .await;
        assert_eq!(reply["result"]["isError"], true);
        let parsed: Value =
            serde_json::from_str(reply["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
        assert!(parsed["error"].as_str().unwrap().contains("no host"));
    }

    #[tokio::test]
    async fn a_fleet_reports_a_bad_host_in_band() {
        // A fleet run does not fail as a whole; each host's outcome, error
        // included, is in the results array.
        let server = writable_server(Vault::in_memory().unwrap());
        let reply = call(
            &server,
            "harbour_run_fleet",
            json!({ "hosts": ["ghost"], "command": "id" }),
        )
        .await;
        assert_eq!(reply["result"]["isError"], false);
        let parsed: Value =
            serde_json::from_str(reply["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
        assert!(parsed["results"][0]["error"]
            .as_str()
            .unwrap()
            .contains("no host"));
    }

    #[tokio::test]
    async fn a_host_by_name_resolves_and_reaches_the_auth_check() {
        // A host with no auth method enabled: resolution and chain succeed, and
        // execution stops at the auth check - proving the name resolved without
        // needing a live server to connect to.
        let vault = Vault::in_memory().unwrap();
        vault
            .create_host(HostInput {
                auth: HostAuth {
                    use_agent: false,
                    key_path: None,
                    use_password: false,
                },
                ..host_input("bare")
            })
            .unwrap();
        let server = writable_server(vault);

        let reply = call(
            &server,
            "harbour_run_command",
            json!({ "host": "bare", "command": "id" }),
        )
        .await;
        assert_eq!(reply["result"]["isError"], true);
        let parsed: Value =
            serde_json::from_str(reply["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
        assert!(parsed["error"]
            .as_str()
            .unwrap()
            .contains("no authentication method"));
    }
}
