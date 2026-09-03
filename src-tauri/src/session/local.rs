//! Local pseudo-terminal sessions (ConPTY on Windows, forkpty elsewhere).

use std::io::{Read, Write};
use std::path::PathBuf;

use portable_pty::{native_pty_system, ChildKiller, CommandBuilder, MasterPty, PtySize};
use tokio::sync::mpsc;

use crate::error::{AppError, AppResult};
use crate::session::shell::ShellSpec;

/// How many raw pty reads may queue before the reader thread blocks. The pump
/// drains this quickly; the real throttle is the ack budget in `reader.rs`.
const READ_QUEUE_DEPTH: usize = 256;
/// Size of a single `read()` from the pty.
const READ_CHUNK: usize = 64 * 1024;

pub struct SpawnOptions<'a> {
    pub shell: &'a ShellSpec,
    pub cols: u16,
    pub rows: u16,
    pub cwd: Option<PathBuf>,
    pub env: Vec<(String, String)>,
}

/// The live handles for a spawned pty, ready to be owned by a `SessionHandle`.
pub struct SpawnedPty {
    pub master: Box<dyn MasterPty + Send>,
    pub writer: Box<dyn Write + Send>,
    pub killer: Box<dyn ChildKiller + Send + Sync>,
    pub output: mpsc::Receiver<Vec<u8>>,
}

/// Spawns `opts.shell` on a new pty.
///
/// `on_exit` fires on a dedicated thread once the child is reaped, carrying the
/// exit code when the platform reports one.
pub fn spawn<F>(opts: SpawnOptions<'_>, on_exit: F) -> AppResult<SpawnedPty>
where
    F: FnOnce(Option<u32>) + Send + 'static,
{
    let pty_system = native_pty_system();
    let pair = pty_system
        .openpty(PtySize {
            rows: opts.rows.max(1),
            cols: opts.cols.max(1),
            pixel_width: 0,
            pixel_height: 0,
        })
        .map_err(|err| AppError::PtyOpen(err.to_string()))?;

    let mut cmd = CommandBuilder::new(&opts.shell.program);
    for arg in &opts.shell.args {
        cmd.arg(arg);
    }
    if let Some(cwd) = opts.cwd.as_ref() {
        if cwd.is_dir() {
            cmd.cwd(cwd);
        }
    }
    for (key, value) in &opts.env {
        cmd.env(key, value);
    }
    // Advertise the emulator we actually implement, so remote-ish programs run
    // through WSL pick sane capabilities.
    cmd.env("TERM", "xterm-256color");

    let mut child = pair
        .slave
        .spawn_command(cmd)
        .map_err(|err| AppError::Spawn {
            program: opts.shell.program.clone(),
            reason: err.to_string(),
        })?;

    // The slave side must be dropped here: while we hold it open the pty never
    // reports EOF, so the reader thread would hang after the child exits.
    drop(pair.slave);

    let killer = child.clone_killer();
    let mut reader = pair
        .master
        .try_clone_reader()
        .map_err(|err| AppError::PtyOpen(err.to_string()))?;
    let writer = pair
        .master
        .take_writer()
        .map_err(|err| AppError::PtyOpen(err.to_string()))?;

    let (tx, output) = mpsc::channel::<Vec<u8>>(READ_QUEUE_DEPTH);

    std::thread::Builder::new()
        .name("harbour-pty-read".into())
        .spawn(move || {
            let mut buf = vec![0u8; READ_CHUNK];
            let mut listening = true;
            loop {
                match reader.read(&mut buf) {
                    Ok(0) => break,
                    Ok(n) => {
                        if !listening {
                            continue;
                        }
                        // Blocks when the queue is full, which is exactly the
                        // backpressure we want to push down to the pty.
                        if tx.blocking_send(buf[..n].to_vec()).is_err() {
                            // Nobody is listening any more, but we must keep
                            // draining: closing a ConPTY blocks until its
                            // output buffer has been consumed, so stopping here
                            // would wedge whichever thread drops the master.
                            listening = false;
                        }
                    }
                    Err(err) if err.kind() == std::io::ErrorKind::Interrupted => continue,
                    Err(_) => break,
                }
            }
        })
        .map_err(AppError::Io)?;

    std::thread::Builder::new()
        .name("harbour-pty-wait".into())
        .spawn(move || {
            let code = child.wait().ok().map(|status| status.exit_code());
            on_exit(code);
        })
        .map_err(AppError::Io)?;

    Ok(SpawnedPty {
        master: pair.master,
        writer,
        killer,
        output,
    })
}
