//! A running SSH shell channel, seen as a [`Transport`].
//!
//! Two tasks drive the channel. The reader turns `ChannelMsg`s back into the
//! same `Receiver<Vec<u8>>` a pty produces, so everything downstream - the
//! batching pump, the ack budget, the terminal - is shared with local
//! sessions. The writer owns the write half *and the session handle*, because
//! dropping the handle is what closes the connection.
//!
//! Nothing in here blocks: `write`, `resize` and `kill` all just queue a
//! command, which is what lets a session be torn down without waiting on a
//! remote that has stopped answering.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use russh::client::{Handle, Handler, Msg};
use russh::{Channel, ChannelMsg, Disconnect};
use tokio::sync::{mpsc, oneshot};

use crate::error::{AppError, AppResult};
use crate::session::{ExitReason, Transport};

/// How many raw channel reads may queue before the reader task stops pulling.
/// Matching the pty path keeps the two transports behaving alike under load;
/// the real throttle is the ack budget in `session::reader`.
const READ_QUEUE_DEPTH: usize = 256;

/// Work for the writer task, kept in one queue so a resize cannot overtake
/// input the user typed before it.
enum Command {
    Data(Vec<u8>),
    Resize {
        cols: u16,
        rows: u16,
    },
    /// Open another channel on the same connection - for SFTP. The writer
    /// task owns the connection handle, so this is where it has to happen.
    OpenChannel(oneshot::Sender<Result<Channel<Msg>, russh::Error>>),
    /// Open a `direct-tcpip` channel to `host:port` - for a local port
    /// forward. Same reason: the connection lives in the writer task.
    OpenForward {
        host: String,
        port: u16,
        reply: oneshot::Sender<Result<Channel<Msg>, russh::Error>>,
    },
    Close,
}

/// A way to open further channels on a running connection.
///
/// It is a handle on the writer task's queue rather than on the connection
/// itself, so the connection keeps exactly one owner and closing the terminal
/// still closes everything that was riding on it.
#[derive(Debug, Clone)]
pub struct ChannelOpener {
    commands: mpsc::UnboundedSender<Command>,
}

impl ChannelOpener {
    pub async fn open(&self) -> AppResult<Channel<Msg>> {
        let (reply, opened) = oneshot::channel();
        self.commands
            .send(Command::OpenChannel(reply))
            .map_err(|_| AppError::SshChannel("the connection is closed".into()))?;
        opened
            .await
            .map_err(|_| {
                AppError::SshChannel("the connection closed while opening a channel".into())
            })?
            .map_err(|err| AppError::SshChannel(err.to_string()))
    }

    /// Opens a `direct-tcpip` channel to `host:port` over this connection, for
    /// a local port forward: one such channel carries one accepted connection.
    pub async fn open_forward(&self, host: &str, port: u16) -> AppResult<Channel<Msg>> {
        let (reply, opened) = oneshot::channel();
        self.commands
            .send(Command::OpenForward {
                host: host.to_string(),
                port,
                reply,
            })
            .map_err(|_| AppError::SshChannel("the connection is closed".into()))?;
        opened
            .await
            .map_err(|_| {
                AppError::SshChannel("the connection closed while opening a forward".into())
            })?
            .map_err(|err| AppError::SshChannel(err.to_string()))
    }
}

#[derive(Debug)]
pub struct SshTransport {
    commands: mpsc::UnboundedSender<Command>,
    /// Set by `kill`, read by the reader task. Without it a session the user
    /// closed and a connection that died look identical from the far end of
    /// the channel: both simply stop.
    closing: Arc<AtomicBool>,
}

impl SshTransport {
    /// For opening SFTP on this connection. Cheap to clone and to hold.
    pub fn opener(&self) -> ChannelOpener {
        ChannelOpener {
            commands: self.commands.clone(),
        }
    }

    fn send(&self, command: Command) -> AppResult<()> {
        self.commands
            .send(command)
            .map_err(|_| AppError::SshChannel("the connection is closed".into()))
    }
}

impl Transport for SshTransport {
    /// Queues input. The queue is unbounded because the producer is a person
    /// typing or pasting: it is bounded in practice, and the alternative -
    /// blocking a command handler until the remote window opens - is worse.
    fn write(&self, data: &[u8]) -> AppResult<()> {
        self.send(Command::Data(data.to_vec()))
    }

    fn resize(&self, cols: u16, rows: u16) -> AppResult<()> {
        self.send(Command::Resize {
            cols: cols.max(1),
            rows: rows.max(1),
        })
    }

    fn kill(&self) {
        self.closing.store(true, Ordering::Release);
        let _ = self.send(Command::Close);
    }
}

/// A channel that has been turned into a session's two halves.
#[derive(Debug)]
pub struct Running {
    pub transport: SshTransport,
    pub output: mpsc::Receiver<Vec<u8>>,
}

/// Starts the reader and writer tasks for an open shell channel.
///
/// `pending` is output that arrived while the channel requests were still
/// being acknowledged - usually the shell's opening prompt - and is delivered
/// ahead of everything else.
///
/// `on_exit` fires once the channel is finished, carrying the remote exit
/// status when the server sent one.
pub fn start<H, F>(
    session: Handle<H>,
    channel: Channel<Msg>,
    pending: Vec<u8>,
    keep_alive: Box<dyn std::any::Any + Send>,
    on_exit: F,
) -> Running
where
    H: Handler + 'static,
    F: FnOnce(ExitReason, Option<u32>) + Send + 'static,
{
    let (read_half, write_half) = channel.split();
    let (bytes_tx, output) = mpsc::channel::<Vec<u8>>(READ_QUEUE_DEPTH);
    let (commands, mut command_rx) = mpsc::unbounded_channel::<Command>();
    let closing = Arc::new(AtomicBool::new(false));
    let reader_closing = Arc::clone(&closing);

    tauri::async_runtime::spawn(async move {
        let mut read_half = read_half;
        let mut exit_code = None;
        let mut listening = true;

        if !pending.is_empty() {
            // The queue is empty and its depth is far greater than one, so
            // this cannot block; it only has to happen before any later read.
            let _ = bytes_tx.send(pending).await;
        }

        while let Some(message) = read_half.wait().await {
            match message {
                // stdout, and stderr: an interactive shell expects both on the
                // same screen, exactly as a local pty merges them.
                ChannelMsg::Data { data } | ChannelMsg::ExtendedData { data, .. } => {
                    if !listening {
                        continue;
                    }
                    // Awaiting here is the backpressure: while the pump is
                    // behind, we stop draining the channel, russh stops
                    // adjusting the window, and the remote stops sending.
                    if bytes_tx.send(data.to_vec()).await.is_err() {
                        listening = false;
                    }
                }
                ChannelMsg::ExitStatus { exit_status } => exit_code = Some(exit_status),
                // A remote killed by a signal has no exit status. Report the
                // signal on the terminal, since nothing else will.
                ChannelMsg::ExitSignal {
                    signal_name,
                    error_message,
                    ..
                } => {
                    let note = format!("\r\n[remote process killed by {signal_name:?}]\r\n");
                    tracing::debug!(signal = ?signal_name, message = %error_message, "remote exited on a signal");
                    if listening {
                        let _ = bytes_tx.send(note.into_bytes()).await;
                    }
                }
                ChannelMsg::Eof | ChannelMsg::Close => break,
                _ => {}
            }
        }

        let reason = match exit_code {
            // The remote shell told us how it ended, which is the truth
            // whatever else was going on.
            Some(_) => ExitReason::Exited,
            None if reader_closing.load(Ordering::Acquire) => ExitReason::Killed,
            // No status and nobody asked for this: the link died, the remote
            // was killed by a signal, or sshd went away.
            None => ExitReason::Lost,
        };
        on_exit(reason, exit_code);
    });

    tauri::async_runtime::spawn(async move {
        // Both halves live here so the connection outlives this task by
        // exactly nothing: when the queue closes, the session goes with it.
        let session = session;
        let write_half = write_half;
        // Holds the jump-host connections for a tunnelled session; never
        // touched, only dropped, which tears the chain down with the session.
        let _keep_alive = keep_alive;

        while let Some(command) = command_rx.recv().await {
            let result = match command {
                Command::Data(bytes) => write_half.data_bytes(bytes).await,
                Command::Resize { cols, rows } => {
                    write_half
                        .window_change(cols as u32, rows as u32, 0, 0)
                        .await
                }
                Command::OpenChannel(reply) => {
                    // The asker may have gone away; that is its business.
                    let _ = reply.send(session.channel_open_session().await);
                    Ok(())
                }
                Command::OpenForward { host, port, reply } => {
                    let opened = session
                        .channel_open_direct_tcpip(host, u32::from(port), "127.0.0.1", 0)
                        .await;
                    let _ = reply.send(opened);
                    Ok(())
                }
                Command::Close => break,
            };
            if let Err(err) = result {
                tracing::debug!(error = %err, "ssh channel write failed");
                break;
            }
        }

        let _ = write_half.close().await;
        let _ = session
            .disconnect(Disconnect::ByApplication, "closed by the user", "")
            .await;
    });

    Running {
        transport: SshTransport { commands, closing },
        output,
    }
}
