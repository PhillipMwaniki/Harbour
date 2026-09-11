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

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use serde::Deserialize;
use serde_json::{json, Value};

use harbour_lib::error::AppResult;
use harbour_lib::ssh::client::{self, Endpoint};
use harbour_lib::ssh::forward::{ForwardSpec, Forwards, RemoteSpec};
use harbour_lib::ssh::known_hosts::KnownHosts;
use harbour_lib::ssh::sftp::{self, SftpSession};
use harbour_lib::ssh::transport::SshTransport;
use harbour_lib::ssh::{
    Asker, HostKeyAnswer, HostKeyQuestion, SecretAnswer, SecretKind, SecretQuestion,
};
use harbour_lib::vault::model::{Host, HostId};
use harbour_lib::vault::secrets::{SecretSlot, SecretStore};
use harbour_lib::vault::store::Vault;

/// How many hosts a fleet run connects to at once - the fleet runner's bound.
const FLEET_CONCURRENCY: usize = 8;

/// The default cap on an SFTP read, when the caller names none: enough for
/// config and text files, not for hauling large files (the app's transfers do
/// that). One mebibyte.
const DEFAULT_READ_LIMIT: u64 = 1024 * 1024;

/// The endpoints a connection to one host needs, plus its display name.
struct Prepared {
    name: String,
    dest: Endpoint,
    jumps: Vec<Endpoint>,
}

/// A required string argument, or a message naming what was missing.
fn string_arg(args: &Value, key: &str) -> Result<String, String> {
    args.get(key)
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| format!("`{key}` is required"))
}

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
    /// The port forwards the server is holding open, and the connection behind
    /// each. A forward lives until it is closed: the engine runs its accept
    /// loop, and the transport in [`Server::connections`] keeps the connection.
    forwards: Arc<Forwards>,
    /// The connection keeping each open forward alive, by forward id. Dropping
    /// one closes its connection, so a closed forward's entry is removed.
    connections: Arc<Mutex<HashMap<String, SshTransport>>>,
    /// Whether tools that act - run commands, write files, open forwards - are
    /// offered. Off by default: an agent can see the estate but not touch it
    /// until the server is started with `--allow-write`.
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
            forwards: Forwards::new(Arc::new(|_| {})),
            connections: Arc::new(Mutex::new(HashMap::new())),
            allow_write: false,
        }
    }

    /// A server over the full core, with execution enabled or not.
    pub fn with_core(core: Core, allow_write: bool) -> Self {
        Self {
            vault: core.vault,
            known_hosts: core.known_hosts,
            secrets: core.secrets,
            // The engine reports through events in the app; the MCP has no event
            // stream, so it reads state back with forward_list instead.
            forwards: Forwards::new(Arc::new(|_| {})),
            connections: Arc::new(Mutex::new(HashMap::new())),
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
            "harbour_sftp_list" => self.sftp_list(arguments).await,
            "harbour_sftp_read" => self.sftp_read(arguments).await,
            "harbour_sftp_write" => self.sftp_write(arguments).await,
            "harbour_forward_open" => self.forward_open(arguments).await,
            "harbour_forward_list" => self.forward_list(),
            "harbour_forward_close" => self.forward_close(arguments),
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
                    "tabColor": host.tab_color,
                    "sftpOnly": host.sftp_only,
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
        let prepared = match self.prepare(spec).await {
            Ok(prepared) => prepared,
            Err(err) => return ExecResult::failed(spec, err),
        };
        let Prepared { name, dest, jumps } = prepared;

        match client::run_command(jumps, dest, Arc::clone(&self.known_hosts), command).await {
            Ok(outcome) => ExecResult {
                name,
                exit_code: outcome.exit_code,
                stdout: String::from_utf8_lossy(&outcome.stdout).into_owned(),
                stderr: String::from_utf8_lossy(&outcome.stderr).into_owned(),
                error: None,
            },
            Err(err) => ExecResult::failed(&name, err.to_string()),
        }
    }

    /// Resolves a host into the endpoints a connection needs, refusing along the
    /// way anything an unattended run must not do: an unknown host, a guarded
    /// one, or one with no authentication method. Shared by the command and
    /// SFTP tools, so both reach a host under the same rules.
    async fn prepare(&self, spec: &str) -> Result<Prepared, String> {
        let host = self.resolve_host(spec).await?;
        // A guarded host expects a person to confirm destructive actions; there
        // is no one to ask over MCP, so it is refused rather than touched blind.
        if host.guarded {
            return Err(format!(
                "`{}` is guarded; act on it from the Harbour app, where it can be confirmed",
                host.name
            ));
        }
        let chain = self.resolve_chain(&host.id).await?;
        for hop in &chain {
            if hop.auth.methods().is_empty() {
                return Err(format!("{} has no authentication method enabled", hop.name));
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
        Ok(Prepared {
            name: host.name,
            dest,
            jumps,
        })
    }

    /// Opens SFTP on a fresh headless connection to `spec`. The returned
    /// transport keeps the connection alive: hold it for the operation, and
    /// dropping it closes the connection.
    async fn open_sftp(&self, spec: &str) -> Result<(SftpSession, SshTransport), String> {
        let Prepared { dest, jumps, .. } = self.prepare(spec).await?;
        let (transport, _remote_forwards) =
            client::connect_headless(jumps, dest, Arc::clone(&self.known_hosts))
                .await
                .map_err(|err| err.to_string())?;
        let sftp = sftp::open(&transport.opener())
            .await
            .map_err(|err| err.to_string())?;
        Ok((sftp, transport))
    }

    async fn sftp_list(&self, args: Value) -> Result<Value, String> {
        if !self.allow_write {
            return Err(WRITE_DISABLED.into());
        }
        let host = string_arg(&args, "host")?;
        let path = args.get("path").and_then(Value::as_str).unwrap_or(".");
        let (sftp, _keep) = self.open_sftp(&host).await?;
        let listing = sftp::list(&sftp, path)
            .await
            .map_err(|err| err.to_string())?;
        serde_json::to_value(&listing).map_err(|err| err.to_string())
    }

    async fn sftp_read(&self, args: Value) -> Result<Value, String> {
        if !self.allow_write {
            return Err(WRITE_DISABLED.into());
        }
        let host = string_arg(&args, "host")?;
        let path = string_arg(&args, "path")?;
        let max_bytes = args
            .get("maxBytes")
            .and_then(Value::as_u64)
            .unwrap_or(DEFAULT_READ_LIMIT);

        let (sftp, _keep) = self.open_sftp(&host).await?;
        let bytes = sftp::read_file(&sftp, &path, max_bytes)
            .await
            .map_err(|err| err.to_string())?;
        // Text if it is valid UTF-8; base64 otherwise, so any file round-trips.
        Ok(match String::from_utf8(bytes) {
            Ok(text) => json!({ "path": path, "encoding": "utf-8", "content": text }),
            Err(err) => json!({
                "path": path,
                "encoding": "base64",
                "content": data_encoding::BASE64.encode(&err.into_bytes()),
            }),
        })
    }

    async fn sftp_write(&self, args: Value) -> Result<Value, String> {
        if !self.allow_write {
            return Err(WRITE_DISABLED.into());
        }
        let host = string_arg(&args, "host")?;
        let path = string_arg(&args, "path")?;
        let content = string_arg(&args, "content")?;
        let data = if args.get("base64").and_then(Value::as_bool).unwrap_or(false) {
            data_encoding::BASE64
                .decode(content.as_bytes())
                .map_err(|err| format!("`content` is not valid base64: {err}"))?
        } else {
            content.into_bytes()
        };

        let (sftp, _keep) = self.open_sftp(&host).await?;
        sftp::write_file(&sftp, &path, &data)
            .await
            .map_err(|err| err.to_string())?;
        Ok(json!({ "path": path, "bytesWritten": data.len() }))
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

    /// A cheap clone that shares the same state - every field is an `Arc` or a
    /// `Copy`. For handing a spawned fleet task its own handle.
    fn clone_refs(&self) -> Self {
        Self {
            vault: Arc::clone(&self.vault),
            known_hosts: Arc::clone(&self.known_hosts),
            secrets: Arc::clone(&self.secrets),
            forwards: Arc::clone(&self.forwards),
            connections: Arc::clone(&self.connections),
            allow_write: self.allow_write,
        }
    }

    async fn forward_open(&self, args: Value) -> Result<Value, String> {
        if !self.allow_write {
            return Err(WRITE_DISABLED.into());
        }
        let host = string_arg(&args, "host")?;
        let kind = args.get("kind").and_then(Value::as_str).unwrap_or("local");
        let bind_address = args
            .get("bindAddress")
            .and_then(Value::as_str)
            .unwrap_or(if kind == "remote" {
                "localhost"
            } else {
                "127.0.0.1"
            })
            .to_string();
        // The port to listen on: local for -L/-D, the server's for -R. 0 asks
        // for a free one, reported back in the result.
        let listen_port = args.get("listenPort").and_then(Value::as_u64).unwrap_or(0) as u16;

        let Prepared { jumps, dest, .. } = self.prepare(&host).await?;
        let (transport, remote_forwards) =
            client::connect_headless(jumps, dest, Arc::clone(&self.known_hosts))
                .await
                .map_err(|err| err.to_string())?;
        let opener = transport.opener();
        // Each forward gets its own connection, so its own session id.
        let session_id = uuidish();

        let info = match kind {
            "local" => {
                let (target_host, target_port) = target(&args)?;
                self.forwards
                    .open_local(
                        session_id,
                        opener,
                        ForwardSpec {
                            bind_address,
                            local_port: listen_port,
                            host: target_host,
                            port: target_port,
                        },
                    )
                    .await
            }
            "dynamic" => {
                self.forwards
                    .open_dynamic(session_id, opener, bind_address, listen_port)
                    .await
            }
            "remote" => {
                let (target_host, target_port) = target(&args)?;
                self.forwards
                    .open_remote(
                        session_id,
                        opener,
                        remote_forwards,
                        RemoteSpec {
                            bind_address,
                            remote_port: listen_port,
                            host: target_host,
                            port: target_port,
                        },
                    )
                    .await
            }
            other => {
                return Err(format!(
                    "`kind` must be local, dynamic or remote, not `{other}`"
                ))
            }
        };

        match info {
            Ok(info) => {
                // Hold the connection open for the life of the forward.
                self.connections
                    .lock()
                    .unwrap()
                    .insert(info.id.clone(), transport);
                serde_json::to_value(&info).map_err(|err| err.to_string())
            }
            // `transport` drops here, closing the connection the forward never
            // got to use.
            Err(err) => Err(err.to_string()),
        }
    }

    fn forward_list(&self) -> Result<Value, String> {
        serde_json::to_value(self.forwards.list()).map_err(|err| err.to_string())
    }

    fn forward_close(&self, args: Value) -> Result<Value, String> {
        let id = string_arg(&args, "id")?;
        self.forwards.close(&id).map_err(|err| err.to_string())?;
        // Drop the connection now the forward that rode it is gone.
        self.connections.lock().unwrap().remove(&id);
        Ok(json!({ "closed": id }))
    }
}

/// The `targetHost`/`targetPort` a local or remote forward delivers to.
fn target(args: &Value) -> Result<(String, u16), String> {
    let host = string_arg(args, "targetHost")?;
    let port = args
        .get("targetPort")
        .and_then(Value::as_u64)
        .ok_or("`targetPort` is required")?;
    Ok((host, port as u16))
}

/// A random-enough id for a per-forward session. Avoids a uuid dependency: the
/// id only has to be unique among this process's live forwards.
fn uuidish() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("mcp-fwd-{nanos}")
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
        tools.push(json!({
            "name": "harbour_sftp_list",
            "description": "List a directory on a saved host over SFTP. Same connection rules as harbour_run_command; guarded hosts are refused.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "host": { "type": "string", "description": "A saved host's id or unique name." },
                    "path": { "type": "string", "description": "The remote directory. Defaults to the login directory." },
                },
                "required": ["host"],
                "additionalProperties": false,
            },
        }));
        tools.push(json!({
            "name": "harbour_sftp_read",
            "description": "Read a file on a saved host over SFTP. Returns text when the file is UTF-8, otherwise base64. Refuses files larger than maxBytes (default 1 MiB) - use the Harbour app to move large files.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "host": { "type": "string" },
                    "path": { "type": "string", "description": "The remote file." },
                    "maxBytes": { "type": "integer", "description": "Refuse a file larger than this. Default 1048576." },
                },
                "required": ["host", "path"],
                "additionalProperties": false,
            },
        }));
        tools.push(json!({
            "name": "harbour_sftp_write",
            "description": "Write a file on a saved host over SFTP, creating or truncating it. Pass base64:true to write binary content given as base64.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "host": { "type": "string" },
                    "path": { "type": "string", "description": "The remote file." },
                    "content": { "type": "string", "description": "The file's contents (text, or base64 when base64 is true)." },
                    "base64": { "type": "boolean", "description": "Whether content is base64-encoded binary. Default false." },
                },
                "required": ["host", "path", "content"],
                "additionalProperties": false,
            },
        }));
        tools.push(json!({
            "name": "harbour_forward_open",
            "description": "Open a port forward over a saved host's connection, held until closed. kind 'local' (-L) listens locally and delivers to targetHost:targetPort reached from the host; 'dynamic' (-D) is a local SOCKS5 proxy; 'remote' (-R) asks the host to listen and delivers to targetHost:targetPort reached from this machine. Returns the forward, including the bound port when listenPort was 0.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "host": { "type": "string", "description": "A saved host's id or unique name." },
                    "kind": { "type": "string", "enum": ["local", "dynamic", "remote"], "description": "Default local." },
                    "bindAddress": { "type": "string", "description": "Where to listen. Default 127.0.0.1 (localhost for remote)." },
                    "listenPort": { "type": "integer", "description": "The port to listen on; 0 (default) picks a free one, reported back." },
                    "targetHost": { "type": "string", "description": "The delivery host, for local and remote forwards." },
                    "targetPort": { "type": "integer", "description": "The delivery port, for local and remote forwards." },
                },
                "required": ["host"],
                "additionalProperties": false,
            },
        }));
        tools.push(json!({
            "name": "harbour_forward_list",
            "description": "List the port forwards the server is currently holding open, with their bound ports and connection counts.",
            "inputSchema": { "type": "object", "properties": {}, "additionalProperties": false },
        }));
        tools.push(json!({
            "name": "harbour_forward_close",
            "description": "Close a port forward by its id, releasing the connection behind it.",
            "inputSchema": {
                "type": "object",
                "properties": { "id": { "type": "string" } },
                "required": ["id"],
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
            tab_color: None,
            sftp_only: false,
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
                tab_color: None,
                sftp_only: false,
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
    async fn sftp_tools_appear_only_with_allow_write() {
        let writable = writable_server(Vault::in_memory().unwrap());
        let list = writable
            .handle(incoming("tools/list", Some(json!(1)), Value::Null))
            .await
            .unwrap();
        let names: Vec<String> = list["result"]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .map(|tool| tool["name"].as_str().unwrap().to_string())
            .collect();
        for tool in [
            "harbour_sftp_list",
            "harbour_sftp_read",
            "harbour_sftp_write",
        ] {
            assert!(names.iter().any(|n| n == tool), "missing {tool}");
        }

        // Hidden and refused on a read-only server.
        let read_only = server_with_a_host();
        let reply = call(
            &read_only,
            "harbour_sftp_read",
            json!({ "host": "web", "path": "/etc/hostname" }),
        )
        .await;
        assert_eq!(reply["result"]["isError"], true);
        assert!(reply["result"]["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("--allow-write"));
    }

    #[tokio::test]
    async fn sftp_refuses_a_guarded_host_before_connecting() {
        let vault = Vault::in_memory().unwrap();
        vault
            .create_host(HostInput {
                guarded: true,
                tab_color: None,
                sftp_only: false,
                ..host_input("prod")
            })
            .unwrap();
        let server = writable_server(vault);

        let reply = call(
            &server,
            "harbour_sftp_list",
            json!({ "host": "prod", "path": "/" }),
        )
        .await;
        assert_eq!(reply["result"]["isError"], true);
        let parsed: Value =
            serde_json::from_str(reply["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
        assert!(parsed["error"].as_str().unwrap().contains("guarded"));
    }

    #[tokio::test]
    async fn sftp_write_needs_its_arguments() {
        let server = writable_server(Vault::in_memory().unwrap());
        let reply = call(&server, "harbour_sftp_write", json!({ "host": "x" })).await;
        assert_eq!(reply["result"]["isError"], true);
        let parsed: Value =
            serde_json::from_str(reply["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
        assert!(parsed["error"].as_str().unwrap().contains("`path`"));
    }

    #[tokio::test]
    async fn forward_tools_appear_only_with_allow_write() {
        let writable = writable_server(Vault::in_memory().unwrap());
        let list = writable
            .handle(incoming("tools/list", Some(json!(1)), Value::Null))
            .await
            .unwrap();
        let names: Vec<String> = list["result"]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .map(|tool| tool["name"].as_str().unwrap().to_string())
            .collect();
        for tool in [
            "harbour_forward_open",
            "harbour_forward_list",
            "harbour_forward_close",
        ] {
            assert!(names.iter().any(|n| n == tool), "missing {tool}");
        }

        let read_only = server_with_a_host();
        let reply = call(
            &read_only,
            "harbour_forward_open",
            json!({ "host": "web", "kind": "dynamic" }),
        )
        .await;
        assert_eq!(reply["result"]["isError"], true);
        assert!(reply["result"]["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("--allow-write"));
    }

    #[tokio::test]
    async fn forward_open_refuses_a_guarded_host_before_connecting() {
        let vault = Vault::in_memory().unwrap();
        vault
            .create_host(HostInput {
                guarded: true,
                tab_color: None,
                sftp_only: false,
                ..host_input("prod")
            })
            .unwrap();
        let server = writable_server(vault);

        let reply = call(
            &server,
            "harbour_forward_open",
            json!({ "host": "prod", "kind": "dynamic", "listenPort": 0 }),
        )
        .await;
        assert_eq!(reply["result"]["isError"], true);
        let parsed: Value =
            serde_json::from_str(reply["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
        assert!(parsed["error"].as_str().unwrap().contains("guarded"));
    }

    #[tokio::test]
    async fn forward_list_starts_empty_and_close_reports_an_unknown_id() {
        let server = writable_server(Vault::in_memory().unwrap());

        let list = call(&server, "harbour_forward_list", json!({})).await;
        assert_eq!(list["result"]["isError"], false);
        let parsed: Value =
            serde_json::from_str(list["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
        assert_eq!(parsed.as_array().unwrap().len(), 0);

        let closed = call(&server, "harbour_forward_close", json!({ "id": "nope" })).await;
        assert_eq!(closed["result"]["isError"], true);
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
