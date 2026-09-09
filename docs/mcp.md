# Harbour MCP server

`harbour-mcp` is a [Model Context Protocol](https://modelcontextprotocol.io)
server that exposes Harbour's saved hosts to an AI agent. It reuses Harbour's
core - the same vault, `known_hosts` and keychain the desktop app uses - with no
GUI, and speaks MCP over stdio.

An agent gets exactly what the person who launched the server has: their saved
hosts, and (with execution enabled) the ability to run commands on them with the
saved credentials. It performs no interactive trust decisions - a host key that
is not already trusted is refused, and a password that is not in the keychain
fails - so an unattended run can never trust a new key or hang on a prompt.

## Building and running

```sh
cargo build --release --manifest-path mcp/Cargo.toml
```

The binary is `harbour-mcp`. It reads Harbour's config from the standard
location (`dirs::config_dir()/com.harbour.app`); set `HARBOUR_CONFIG_DIR` to
point it elsewhere, e.g. at a portable install's directory.

Command execution is **off by default** - the server offers only the read-only
inventory tools. Pass `--allow-write` (or set `HARBOUR_MCP_ALLOW_WRITE=1`) to
enable the tools that act.

## Pointing a client at it

For an MCP client that launches servers over stdio (e.g. Claude Desktop's
`claude_desktop_config.json`), read-only:

```json
{
  "mcpServers": {
    "harbour": {
      "command": "/path/to/harbour-mcp"
    }
  }
}
```

Add `"args": ["--allow-write"]` to let the agent run commands.

## Tools

Read-only (always available):

| Tool | Arguments | Returns |
| --- | --- | --- |
| `harbour_list_hosts` | – | saved hosts: id, name, hostname, port, username, folder, jump host, guarded, whether a password is saved |
| `harbour_list_folders` | – | the folder tree: id, name, parent |

Acting on hosts (only with `--allow-write`):

| Tool | Arguments | Returns |
| --- | --- | --- |
| `harbour_run_command` | `host` (id or unique name), `command` | `{ host, exitCode, stdout, stderr }`, or a tool error if the host could not be reached or run |
| `harbour_run_fleet` | `hosts` (ids or names), `command` | `{ results: [{ host, exitCode, stdout, stderr, error }] }`, one per host |
| `harbour_sftp_list` | `host`, `path` (default: login dir) | the directory listing |
| `harbour_sftp_read` | `host`, `path`, `maxBytes` (default 1 MiB) | `{ path, encoding, content }` — `encoding` is `utf-8`, or `base64` when the file is not valid UTF-8 |
| `harbour_sftp_write` | `host`, `path`, `content`, `base64` (default false) | `{ path, bytesWritten }` |

`harbour_sftp_read` refuses a file larger than `maxBytes` without reading it, and
`harbour_sftp_write` creates or truncates the file. Each SFTP call opens a fresh
connection for the operation and closes it after. Guarded hosts are refused, as
for command execution.

`harbour_run_command` reports a host it could not reach or run on as a tool
error (`isError`); a command that ran and exited non-zero is a success carrying
that `exitCode`. `harbour_run_fleet` never fails as a whole - each host's
outcome, error included, is in the `results` array, at most eight hosts
connecting at once.

## Safety

- **Read-only by default.** Nothing acts until `--allow-write`.
- **Guarded hosts are refused** for execution. A host marked *guarded* in the
  vault expects a person to confirm destructive commands; there is no one to ask
  over MCP, so the run is refused. Run those from the app.
- **No new trust.** Unknown host keys are refused; only keychain-saved passwords
  are used. The server never prompts.
- **Secrets stay in.** No password, key or passphrase ever appears in a tool
  result or in the logs (which go to stderr, never stdout).

## Scope

This is an evolving preview. Port forwarding is planned next; see
[`docs/proposals/harbour-mcp.md`](proposals/harbour-mcp.md).
