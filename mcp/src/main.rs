//! The Harbour MCP server binary: wires the [`Server`] to stdin/stdout.
//!
//! MCP's stdio transport is newline-delimited JSON-RPC. We read a line, parse
//! it, dispatch it, and write the reply (if any) as one line. Logging goes to
//! stderr so it never corrupts the protocol stream on stdout.

use harbour_mcp::{open_vault, Incoming, Server};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

#[tokio::main]
async fn main() -> std::io::Result<()> {
    // Stderr only: stdout is the protocol channel and must carry nothing else.
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_env("HARBOUR_MCP_LOG")
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let server = Server::new(open_vault());
    tracing::info!("harbour mcp server ready on stdio");

    let mut lines = BufReader::new(tokio::io::stdin()).lines();
    let mut stdout = tokio::io::stdout();

    while let Some(line) = lines.next_line().await? {
        if line.trim().is_empty() {
            continue;
        }
        let reply = match serde_json::from_str::<Incoming>(&line) {
            Ok(incoming) => server.handle(incoming).await,
            Err(err) => {
                // A line we cannot parse has no id to answer to; log and move on
                // rather than guessing one.
                tracing::warn!(error = %err, "ignoring an unparseable message");
                continue;
            }
        };
        if let Some(reply) = reply {
            let mut bytes = serde_json::to_vec(&reply)?;
            bytes.push(b'\n');
            stdout.write_all(&bytes).await?;
            stdout.flush().await?;
        }
    }

    Ok(())
}
