//! End-to-end tests for the SSH client, against a real SSH server.
//!
//! The server is a `russh` server started in-process on a loopback port, so
//! these run everywhere CI does, with no fixtures and no network. What they
//! cover is the part unit tests cannot reach: the handshake, host key
//! verification against a store, authentication, the pty and shell requests,
//! and bytes travelling in both directions over a live channel.

use std::collections::{HashMap, VecDeque};
use std::io::{Read, Seek, SeekFrom, Write};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{mpsc as std_mpsc, Arc, Mutex};
use std::time::Duration;

use harbour_lib::edit::Editor;
use harbour_lib::error::AppResult;
use harbour_lib::files::EntryKind;
use harbour_lib::session::{ExitReason, Transport};
use harbour_lib::ssh::client::{self, ConnectRequest};
use harbour_lib::ssh::known_hosts::KnownHosts;
use harbour_lib::ssh::sftp;
use harbour_lib::ssh::{
    Asker, AuthChoice, HostKeyAnswer, HostKeyQuestion, HostKeyStatus, SecretAnswer, SecretKind,
    SecretQuestion, SshTarget,
};
use harbour_lib::transfer::engine::Engine;
use harbour_lib::transfer::{
    ConflictPolicy, Direction, Request, Resolution, Transfer, TransferState,
};
use russh::keys::{Algorithm, PrivateKey};
use russh::server::{Auth, Msg, Server as _, Session};
use russh::{Channel, ChannelId, MethodKind, MethodSet};
use russh_sftp::protocol::{
    Attrs, Data, File, FileAttributes, Handle as SftpHandle, Name, OpenFlags, Status, StatusCode,
};
use tokio::net::TcpListener;
use tokio::sync::mpsc;

const USER: &str = "deploy";
const PASSWORD: &str = "hunter2";
const GREETING: &str = "harbour test shell\r\n$ ";
/// What the fake shell reports when it is asked to exit. Chosen to be
/// distinctive, so a test cannot pass on a coincidental zero.
const EXIT_CODE: u32 = 7;

// ---------------------------------------------------------------------------
// A minimal SSH server
// ---------------------------------------------------------------------------

/// Something the server saw, so a test can assert on what the client sent
/// rather than only on what came back.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Seen {
    Pty { term: String, cols: u32, rows: u32 },
    Resize { cols: u32, rows: u32 },
    Input(Vec<u8>),
}

#[derive(Clone)]
struct TestServer {
    seen: mpsc::UnboundedSender<Seen>,
    /// Channels the client opened, kept until a subsystem request says what
    /// they are for. The shell channel never asks, and stays here harmlessly.
    channels: Arc<Mutex<HashMap<ChannelId, Channel<Msg>>>>,
    /// Served as `/` over SFTP.
    sftp_root: PathBuf,
}

impl russh::server::Server for TestServer {
    type Handler = Self;

    fn new_client(&mut self, _peer: Option<SocketAddr>) -> Self {
        self.clone()
    }
}

impl russh::server::Handler for TestServer {
    type Error = russh::Error;

    async fn auth_password(&mut self, user: &str, password: &str) -> Result<Auth, Self::Error> {
        if user == USER && password == PASSWORD {
            Ok(Auth::Accept)
        } else {
            Ok(Auth::reject())
        }
    }

    async fn channel_open_session(
        &mut self,
        channel: Channel<Msg>,
        reply: russh::server::ChannelOpenHandle,
        _session: &mut Session,
    ) -> Result<(), Self::Error> {
        self.channels.lock().unwrap().insert(channel.id(), channel);
        reply.accept().await;
        Ok(())
    }

    /// `sftp` is the only subsystem on offer, served over the channel that
    /// asked for it. Anything else is refused, the way a locked-down sshd
    /// would.
    async fn subsystem_request(
        &mut self,
        channel_id: ChannelId,
        name: &str,
        session: &mut Session,
    ) -> Result<(), Self::Error> {
        if name != "sftp" {
            return session.channel_failure(channel_id);
        }
        let Some(channel) = self.channels.lock().unwrap().remove(&channel_id) else {
            return session.channel_failure(channel_id);
        };
        session.channel_success(channel_id)?;
        let handler = SftpHandler {
            root: self.sftp_root.clone(),
            handles: HashMap::new(),
            next_handle: 0,
        };
        tokio::spawn(russh_sftp::server::run(channel.into_stream(), handler));
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    async fn pty_request(
        &mut self,
        channel: ChannelId,
        term: &str,
        col_width: u32,
        row_height: u32,
        _pix_width: u32,
        _pix_height: u32,
        _modes: &[(russh::Pty, u32)],
        session: &mut Session,
    ) -> Result<(), Self::Error> {
        let _ = self.seen.send(Seen::Pty {
            term: term.to_string(),
            cols: col_width,
            rows: row_height,
        });
        session.channel_success(channel)
    }

    async fn shell_request(
        &mut self,
        channel: ChannelId,
        session: &mut Session,
    ) -> Result<(), Self::Error> {
        session.channel_success(channel)?;
        session.data(channel, GREETING.as_bytes().to_vec())
    }

    async fn window_change_request(
        &mut self,
        channel: ChannelId,
        col_width: u32,
        row_height: u32,
        _pix_width: u32,
        _pix_height: u32,
        session: &mut Session,
    ) -> Result<(), Self::Error> {
        let _ = self.seen.send(Seen::Resize {
            cols: col_width,
            rows: row_height,
        });
        session.channel_success(channel)
    }

    /// Echoes input, except `exit`, which ends the shell with [`EXIT_CODE`].
    async fn data(
        &mut self,
        channel: ChannelId,
        data: &[u8],
        session: &mut Session,
    ) -> Result<(), Self::Error> {
        let _ = self.seen.send(Seen::Input(data.to_vec()));
        if data.starts_with(b"exit") {
            session.exit_status_request(channel, EXIT_CODE)?;
            session.close(channel)?;
            return Ok(());
        }
        session.data(channel, data.to_vec())
    }
}

struct RunningServer {
    addr: SocketAddr,
    seen: mpsc::UnboundedReceiver<Seen>,
    /// The directory the server's SFTP serves as `/`; a test fills it.
    sftp_root: PathBuf,
}

/// A server offering what a typical host does: a password, and the
/// keyboard-interactive route to one.
async fn start_server() -> RunningServer {
    start_server_offering(&[MethodKind::Password, MethodKind::KeyboardInteractive]).await
}

/// A server that advertises exactly `methods`, so a test can arrange for the
/// client to want something the server will not entertain.
async fn start_server_offering(methods: &[MethodKind]) -> RunningServer {
    let key = PrivateKey::random(&mut rand::rng(), Algorithm::Ed25519).expect("generate host key");

    let mut advertised = MethodSet::empty();
    for method in methods {
        advertised.push(*method);
    }

    let config = Arc::new(russh::server::Config {
        keys: vec![key],
        methods: advertised,
        // The default backs off after a failed attempt, which would make the
        // wrong-password test take seconds for no reason.
        auth_rejection_time: Duration::from_millis(1),
        auth_rejection_time_initial: Some(Duration::from_millis(0)),
        ..Default::default()
    });

    let listener = TcpListener::bind(("127.0.0.1", 0)).await.expect("bind");
    let addr = listener.local_addr().expect("local addr");
    let (seen_tx, seen) = mpsc::unbounded_channel();

    let sftp_root = std::env::temp_dir().join(format!(
        "harbour-sftp-it-{}-{}",
        std::process::id(),
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&sftp_root).expect("sftp root");
    let served = sftp_root.clone();

    tokio::spawn(async move {
        let mut server = TestServer {
            seen: seen_tx,
            channels: Arc::new(Mutex::new(HashMap::new())),
            sftp_root: served,
        };
        let _ = server.run_on_socket(config, &listener).await;
    });

    RunningServer {
        addr,
        seen,
        sftp_root,
    }
}

// ---------------------------------------------------------------------------
// A scripted stand-in for the user
// ---------------------------------------------------------------------------

#[derive(Default)]
struct ScriptedAsker {
    host_key_answer: Mutex<Option<HostKeyAnswer>>,
    secrets: Mutex<VecDeque<SecretAnswer>>,
    host_key_questions: Mutex<Vec<HostKeyQuestion>>,
    secret_questions: Mutex<Vec<SecretQuestion>>,
}

impl ScriptedAsker {
    /// Accepts and remembers the host key, then answers with `password`.
    fn trusting(password: &str) -> Arc<Self> {
        let asker = Self::default();
        *asker.host_key_answer.lock().unwrap() = Some(HostKeyAnswer {
            accept: true,
            remember: true,
        });
        asker.secrets.lock().unwrap().push_back(SecretAnswer {
            secret: Some(password.to_string()),
            remember: false,
        });
        Arc::new(asker)
    }

    fn host_key_questions(&self) -> Vec<HostKeyQuestion> {
        self.host_key_questions.lock().unwrap().clone()
    }

    fn secret_questions(&self) -> Vec<SecretQuestion> {
        self.secret_questions.lock().unwrap().clone()
    }
}

impl Asker for ScriptedAsker {
    async fn host_key(&self, question: HostKeyQuestion) -> AppResult<HostKeyAnswer> {
        self.host_key_questions.lock().unwrap().push(question);
        Ok(self
            .host_key_answer
            .lock()
            .unwrap()
            .unwrap_or(HostKeyAnswer {
                accept: false,
                remember: false,
            }))
    }

    async fn secret(&self, question: SecretQuestion) -> AppResult<SecretAnswer> {
        self.secret_questions.lock().unwrap().push(question);
        Ok(self
            .secrets
            .lock()
            .unwrap()
            .pop_front()
            // Running out of scripted answers means the user gave up, which is
            // exactly what a cancelled prompt looks like.
            .unwrap_or(SecretAnswer {
                secret: None,
                remember: false,
            }))
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn temp_known_hosts() -> KnownHosts {
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let dir = std::env::temp_dir().join(format!(
        "harbour-ssh-it-{}-{}",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir_all(&dir).expect("temp dir");
    let path = dir.join("known_hosts");
    // Only Harbour's own file, so the machine running the tests is not
    // consulted and never written to.
    KnownHosts::with_paths(vec![path.clone()], path)
}

fn request(addr: SocketAddr, methods: Vec<AuthChoice>) -> ConnectRequest {
    ConnectRequest {
        target: SshTarget {
            host: addr.ip().to_string(),
            port: addr.port(),
            user: USER.to_string(),
        },
        methods,
        cols: 100,
        rows: 30,
    }
}

/// Reads batches until `needle` shows up, or gives up after ten seconds.
async fn read_until(rx: &mut mpsc::Receiver<Vec<u8>>, needle: &str) -> String {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    let mut seen = String::new();
    loop {
        let chunk = tokio::time::timeout_at(deadline, rx.recv())
            .await
            .unwrap_or_else(|_| panic!("timed out waiting for {needle:?}; saw {seen:?}"))
            .unwrap_or_else(|| panic!("the stream ended before {needle:?}; saw {seen:?}"));
        seen.push_str(&String::from_utf8_lossy(&chunk));
        if seen.contains(needle) {
            return seen;
        }
    }
}

/// Drains everything the server has recorded so far.
fn drain(seen: &mut mpsc::UnboundedReceiver<Seen>) -> Vec<Seen> {
    let mut out = Vec::new();
    while let Ok(event) = seen.try_recv() {
        out.push(event);
    }
    out
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread")]
async fn connects_authenticates_and_runs_a_remote_shell() {
    let mut server = start_server().await;
    let asker = ScriptedAsker::trusting(PASSWORD);
    let (exit_tx, exit_rx) = std_mpsc::channel();

    let mut connected = client::connect(
        request(server.addr, vec![AuthChoice::Password]),
        Arc::clone(&asker),
        Arc::new(temp_known_hosts()),
        move |reason, code| {
            let _ = exit_tx.send((reason, code));
        },
    )
    .await
    .expect("the connection should succeed");

    assert_eq!(connected.method, "password");
    assert!(connected.fingerprint.starts_with("SHA256:"));

    // The shell's opening output must survive the channel request round trip.
    let greeting = read_until(&mut connected.output, "harbour test shell").await;
    assert!(
        greeting.contains("$ "),
        "expected a prompt, saw {greeting:?}"
    );

    connected.transport.write(b"echo hello\r").unwrap();
    let echoed = read_until(&mut connected.output, "echo hello").await;
    assert!(echoed.contains("echo hello"));

    connected.transport.resize(132, 45).unwrap();
    connected.transport.write(b"exit\r").unwrap();

    let (reason, code) = exit_rx
        .recv_timeout(Duration::from_secs(10))
        .expect("the session should report that it ended");
    assert_eq!(reason, ExitReason::Exited);
    assert_eq!(code, Some(EXIT_CODE));

    let events = drain(&mut server.seen);
    assert!(
        events.contains(&Seen::Pty {
            term: "xterm-256color".into(),
            cols: 100,
            rows: 30,
        }),
        "the pty must be requested at the size the terminal measured; saw {events:?}"
    );
    assert!(
        events.contains(&Seen::Resize {
            cols: 132,
            rows: 45
        }),
        "a resize must reach the remote; saw {events:?}"
    );
}

/// Trust on first use: having said "remember", the second connection must not
/// ask again.
#[tokio::test(flavor = "multi_thread")]
async fn a_remembered_host_key_is_not_questioned_twice() {
    let server = start_server().await;
    let known_hosts = Arc::new(temp_known_hosts());

    let first = ScriptedAsker::trusting(PASSWORD);
    let connected = client::connect(
        request(server.addr, vec![AuthChoice::Password]),
        Arc::clone(&first),
        Arc::clone(&known_hosts),
        |_, _| {},
    )
    .await
    .expect("the first connection should succeed");
    assert_eq!(first.host_key_questions().len(), 1);
    connected.transport.kill();

    let second = ScriptedAsker::trusting(PASSWORD);
    let connected = client::connect(
        request(server.addr, vec![AuthChoice::Password]),
        Arc::clone(&second),
        known_hosts,
        |_, _| {},
    )
    .await
    .expect("the second connection should succeed");

    assert!(
        second.host_key_questions().is_empty(),
        "a key the user chose to remember must not be asked about again"
    );
    connected.transport.kill();
}

#[tokio::test(flavor = "multi_thread")]
async fn an_unknown_host_is_described_before_it_is_trusted() {
    let server = start_server().await;
    let asker = ScriptedAsker::trusting(PASSWORD);

    let connected = client::connect(
        request(server.addr, vec![AuthChoice::Password]),
        Arc::clone(&asker),
        Arc::new(temp_known_hosts()),
        |_, _| {},
    )
    .await
    .expect("the connection should succeed");

    let asked = asker.host_key_questions();
    assert_eq!(asked.len(), 1);
    assert_eq!(asked[0].status, HostKeyStatus::Unknown);
    assert_eq!(asked[0].algorithm, "ssh-ed25519");
    assert_eq!(asked[0].fingerprint, connected.fingerprint);
    assert!(
        asked[0].stored.is_empty(),
        "nothing is on file for a host never seen before"
    );
    connected.transport.kill();
}

/// The case the security model exists for: a key on file that does not match.
#[tokio::test(flavor = "multi_thread")]
async fn a_changed_host_key_is_reported_with_both_fingerprints() {
    let server = start_server().await;
    let known_hosts = temp_known_hosts();

    // Record a different key for this host, as if the server had been swapped.
    let impostor = PrivateKey::random(&mut rand::rng(), Algorithm::Ed25519).unwrap();
    known_hosts
        .learn(
            &server.addr.ip().to_string(),
            server.addr.port(),
            impostor.public_key(),
        )
        .unwrap();
    let stored_fingerprint = harbour_lib::ssh::known_hosts::fingerprint(impostor.public_key());

    let asker = ScriptedAsker::trusting(PASSWORD);
    let connected = client::connect(
        request(server.addr, vec![AuthChoice::Password]),
        Arc::clone(&asker),
        Arc::new(known_hosts),
        |_, _| {},
    )
    .await
    .expect("accepting the new key should still connect");

    let asked = asker.host_key_questions();
    assert_eq!(asked.len(), 1);
    assert_eq!(asked[0].status, HostKeyStatus::Changed);
    assert_eq!(asked[0].stored.len(), 1);
    assert_eq!(asked[0].stored[0].fingerprint, stored_fingerprint);
    assert_ne!(asked[0].fingerprint, stored_fingerprint);
    connected.transport.kill();
}

#[tokio::test(flavor = "multi_thread")]
async fn refusing_a_host_key_aborts_the_connection() {
    let server = start_server().await;
    // No scripted answer means the asker refuses.
    let asker = Arc::new(ScriptedAsker::default());

    let error = client::connect(
        request(server.addr, vec![AuthChoice::Password]),
        Arc::clone(&asker),
        Arc::new(temp_known_hosts()),
        |_, _| {},
    )
    .await
    .expect_err("a refused host key must not connect");

    assert_eq!(error.code(), "SSH_HOSTKEY_REJECTED");
    assert!(
        asker.secret_questions().is_empty(),
        "a password must never be asked for on a host that was not trusted"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_wrong_password_is_an_authentication_failure() {
    let server = start_server().await;
    let asker = ScriptedAsker::trusting("not the password");

    let error = client::connect(
        request(server.addr, vec![AuthChoice::Password]),
        asker,
        Arc::new(temp_known_hosts()),
        |_, _| {},
    )
    .await
    .expect_err("the wrong password must not connect");

    assert_eq!(error.code(), "SSH_AUTH_FAILED");
    assert!(
        error.to_string().contains("password"),
        "the message should name what was tried: {error}"
    );
}

/// Dismissing the password prompt is not a failed attempt; it is a decision to
/// stop, and it must not silently roll on to the next method.
#[tokio::test(flavor = "multi_thread")]
async fn cancelling_the_password_prompt_stops_the_attempt() {
    let server = start_server().await;
    let asker = Arc::new(ScriptedAsker::default());
    *asker.host_key_answer.lock().unwrap() = Some(HostKeyAnswer {
        accept: true,
        remember: false,
    });

    let error = client::connect(
        request(
            server.addr,
            vec![AuthChoice::Password, AuthChoice::KeyboardInteractive],
        ),
        Arc::clone(&asker),
        Arc::new(temp_known_hosts()),
        |_, _| {},
    )
    .await
    .expect_err("a cancelled prompt must not connect");

    assert_eq!(error.code(), "SSH_AUTH_FAILED");
    assert!(error.to_string().contains("cancelled"), "{error}");
    assert_eq!(
        asker.secret_questions().len(),
        1,
        "cancelling must stop, not fall through to the next method"
    );
    assert_eq!(asker.secret_questions()[0].kind, SecretKind::Password);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_method_the_server_will_not_take_fails_before_asking_the_user() {
    // A host with `KbdInteractiveAuthentication no`, which is common.
    let server = start_server_offering(&[MethodKind::Password]).await;
    let asker = ScriptedAsker::trusting(PASSWORD);

    let error = client::connect(
        request(server.addr, vec![AuthChoice::KeyboardInteractive]),
        Arc::clone(&asker),
        Arc::new(temp_known_hosts()),
        |_, _| {},
    )
    .await
    .expect_err("a method the server does not offer cannot succeed");

    assert_eq!(error.code(), "SSH_AUTH_FAILED");
    assert!(
        error.to_string().contains("not offered by the server"),
        "the message should say why: {error}"
    );
    assert!(
        asker.secret_questions().is_empty(),
        "nothing should be asked of the user for a method that cannot be used"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_host_that_is_not_listening_fails_to_connect() {
    // Bind and drop, so the port is real but nothing is behind it.
    let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
    let addr = listener.local_addr().unwrap();
    drop(listener);

    let error = client::connect(
        request(addr, vec![AuthChoice::Password]),
        ScriptedAsker::trusting(PASSWORD),
        Arc::new(temp_known_hosts()),
        |_, _| {},
    )
    .await
    .expect_err("there is nothing to connect to");

    assert_eq!(error.code(), "SSH_CONNECT_FAILED");
}

/// Closing a session is the app's decision, and must be reported as such
/// rather than as a connection that failed.
#[tokio::test(flavor = "multi_thread")]
async fn closing_a_session_reports_it_as_killed() {
    let server = start_server().await;
    let (exit_tx, exit_rx) = std_mpsc::channel();

    let mut connected = client::connect(
        request(server.addr, vec![AuthChoice::Password]),
        ScriptedAsker::trusting(PASSWORD),
        Arc::new(temp_known_hosts()),
        move |reason, code| {
            let _ = exit_tx.send((reason, code));
        },
    )
    .await
    .expect("the connection should succeed");

    read_until(&mut connected.output, "harbour test shell").await;
    connected.transport.kill();

    let (reason, code) = exit_rx
        .recv_timeout(Duration::from_secs(10))
        .expect("closing should report an end");
    assert_eq!(reason, ExitReason::Killed);
    assert_eq!(code, None);
}

// ---------------------------------------------------------------------------
// A minimal SFTP server over a temp directory
// ---------------------------------------------------------------------------

/// What an open handle refers to.
enum Opened {
    Dir(VecDeque<File>),
    File(std::fs::File),
}

/// Serves one directory tree as `/`: listing, reading and writing files at
/// offsets, and the handful of mutations the panes and the transfer engine
/// use. Enough of the protocol to be an honest stand-in for OpenSSH's sftp
/// server in tests, and no more.
struct SftpHandler {
    root: PathBuf,
    handles: HashMap<String, Opened>,
    next_handle: u32,
}

fn ok(id: u32) -> Status {
    Status {
        id,
        status_code: StatusCode::Ok,
        error_message: "Ok".into(),
        language_tag: "en".into(),
    }
}

impl SftpHandler {
    /// Normalises a POSIX path under the virtual root: `.` and `` are `/`,
    /// and `..` pops, so the client's canonicalize gets a real answer.
    fn canonical(&self, path: &str) -> String {
        let mut parts: Vec<&str> = Vec::new();
        for part in path.split('/') {
            match part {
                "" | "." => {}
                ".." => {
                    parts.pop();
                }
                other => parts.push(other),
            }
        }
        format!("/{}", parts.join("/"))
    }

    fn resolve(&self, path: &str) -> PathBuf {
        self.canonical(path)
            .split('/')
            .filter(|part| !part.is_empty())
            .fold(self.root.clone(), |dir, part| dir.join(part))
    }
}

impl russh_sftp::server::Handler for SftpHandler {
    type Error = StatusCode;

    fn unimplemented(&self) -> StatusCode {
        StatusCode::OpUnsupported
    }

    async fn realpath(&mut self, id: u32, path: String) -> Result<Name, StatusCode> {
        Ok(Name {
            id,
            files: vec![File::dummy(self.canonical(&path))],
        })
    }

    async fn opendir(&mut self, id: u32, path: String) -> Result<SftpHandle, StatusCode> {
        let read = std::fs::read_dir(self.resolve(&path)).map_err(|_| StatusCode::NoSuchFile)?;
        let mut files = VecDeque::new();
        for entry in read.flatten() {
            let meta = entry.metadata().map_err(|_| StatusCode::Failure)?;
            files.push_back(File::new(
                entry.file_name().to_string_lossy().into_owned(),
                FileAttributes::from(&meta),
            ));
        }
        self.next_handle += 1;
        let handle = self.next_handle.to_string();
        self.handles.insert(handle.clone(), Opened::Dir(files));
        Ok(SftpHandle { id, handle })
    }

    async fn readdir(&mut self, id: u32, handle: String) -> Result<Name, StatusCode> {
        let Some(Opened::Dir(files)) = self.handles.get_mut(&handle) else {
            return Err(StatusCode::NoSuchFile);
        };
        if files.is_empty() {
            return Err(StatusCode::Eof);
        }
        Ok(Name {
            id,
            files: files.drain(..).collect(),
        })
    }

    async fn close(&mut self, id: u32, handle: String) -> Result<Status, StatusCode> {
        self.handles.remove(&handle);
        Ok(ok(id))
    }

    async fn open(
        &mut self,
        id: u32,
        filename: String,
        pflags: OpenFlags,
        _attrs: FileAttributes,
    ) -> Result<SftpHandle, StatusCode> {
        let options: std::fs::OpenOptions = pflags.into();
        let file = options
            .open(self.resolve(&filename))
            .map_err(|_| StatusCode::NoSuchFile)?;
        self.next_handle += 1;
        let handle = self.next_handle.to_string();
        self.handles.insert(handle.clone(), Opened::File(file));
        Ok(SftpHandle { id, handle })
    }

    async fn read(
        &mut self,
        id: u32,
        handle: String,
        offset: u64,
        len: u32,
    ) -> Result<Data, StatusCode> {
        let Some(Opened::File(file)) = self.handles.get_mut(&handle) else {
            return Err(StatusCode::NoSuchFile);
        };
        file.seek(SeekFrom::Start(offset))
            .map_err(|_| StatusCode::Failure)?;
        let mut data = vec![0u8; len as usize];
        let mut filled = 0;
        while filled < data.len() {
            let n = file
                .read(&mut data[filled..])
                .map_err(|_| StatusCode::Failure)?;
            if n == 0 {
                break;
            }
            filled += n;
        }
        if filled == 0 && len > 0 {
            return Err(StatusCode::Eof);
        }
        data.truncate(filled);
        Ok(Data { id, data })
    }

    async fn write(
        &mut self,
        id: u32,
        handle: String,
        offset: u64,
        data: Vec<u8>,
    ) -> Result<Status, StatusCode> {
        let Some(Opened::File(file)) = self.handles.get_mut(&handle) else {
            return Err(StatusCode::NoSuchFile);
        };
        file.seek(SeekFrom::Start(offset))
            .map_err(|_| StatusCode::Failure)?;
        file.write_all(&data).map_err(|_| StatusCode::Failure)?;
        Ok(ok(id))
    }

    async fn fstat(&mut self, id: u32, handle: String) -> Result<Attrs, StatusCode> {
        let Some(Opened::File(file)) = self.handles.get(&handle) else {
            return Err(StatusCode::NoSuchFile);
        };
        let meta = file.metadata().map_err(|_| StatusCode::Failure)?;
        Ok(Attrs {
            id,
            attrs: FileAttributes::from(&meta),
        })
    }

    async fn setstat(
        &mut self,
        id: u32,
        path: String,
        attrs: FileAttributes,
    ) -> Result<Status, StatusCode> {
        if let Some(mtime) = attrs.mtime {
            let when = std::time::UNIX_EPOCH + Duration::from_secs(u64::from(mtime));
            std::fs::File::options()
                .write(true)
                .open(self.resolve(&path))
                .and_then(|file| file.set_modified(when))
                .map_err(|_| StatusCode::NoSuchFile)?;
        }
        Ok(ok(id))
    }

    async fn mkdir(
        &mut self,
        id: u32,
        path: String,
        _attrs: FileAttributes,
    ) -> Result<Status, StatusCode> {
        std::fs::create_dir(self.resolve(&path)).map_err(|_| StatusCode::Failure)?;
        Ok(ok(id))
    }

    async fn remove(&mut self, id: u32, filename: String) -> Result<Status, StatusCode> {
        std::fs::remove_file(self.resolve(&filename)).map_err(|_| StatusCode::NoSuchFile)?;
        Ok(ok(id))
    }

    async fn rmdir(&mut self, id: u32, path: String) -> Result<Status, StatusCode> {
        std::fs::remove_dir(self.resolve(&path)).map_err(|_| StatusCode::Failure)?;
        Ok(ok(id))
    }

    async fn rename(
        &mut self,
        id: u32,
        oldpath: String,
        newpath: String,
    ) -> Result<Status, StatusCode> {
        std::fs::rename(self.resolve(&oldpath), self.resolve(&newpath))
            .map_err(|_| StatusCode::NoSuchFile)?;
        Ok(ok(id))
    }

    async fn stat(&mut self, id: u32, path: String) -> Result<Attrs, StatusCode> {
        let meta = std::fs::metadata(self.resolve(&path)).map_err(|_| StatusCode::NoSuchFile)?;
        Ok(Attrs {
            id,
            attrs: FileAttributes::from(&meta),
        })
    }

    async fn lstat(&mut self, id: u32, path: String) -> Result<Attrs, StatusCode> {
        let meta =
            std::fs::symlink_metadata(self.resolve(&path)).map_err(|_| StatusCode::NoSuchFile)?;
        Ok(Attrs {
            id,
            attrs: FileAttributes::from(&meta),
        })
    }
}

// ---------------------------------------------------------------------------
// SFTP tests
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread")]
async fn sftp_rides_the_terminal_connection_and_lists_directories() {
    let server = start_server().await;
    std::fs::create_dir_all(server.sftp_root.join("projects").join("harbour")).unwrap();
    std::fs::write(server.sftp_root.join("notes.txt"), "hello").unwrap();
    std::fs::write(server.sftp_root.join(".profile"), "").unwrap();

    let asker = ScriptedAsker::trusting(PASSWORD);
    let mut connected = client::connect(
        request(server.addr, vec![AuthChoice::Password]),
        Arc::clone(&asker),
        Arc::new(temp_known_hosts()),
        |_, _| {},
    )
    .await
    .expect("the connection should succeed");
    read_until(&mut connected.output, "$ ").await;

    let sftp = sftp::open(&connected.transport.opener())
        .await
        .expect("the sftp subsystem should open on the same connection");
    assert_eq!(sftp::home(&sftp).await.unwrap(), "/");

    let root = sftp::list(&sftp, "/").await.unwrap();
    assert_eq!(root.path, "/");
    assert_eq!(root.parent, None);
    let find = |name: &str| {
        root.entries
            .iter()
            .find(|entry| entry.name == name)
            .unwrap_or_else(|| panic!("no {name} in {:?}", root.entries))
    };
    assert_eq!(find("projects").kind, EntryKind::Dir);
    assert_eq!(find("projects").size, None);
    assert_eq!(find("notes.txt").kind, EntryKind::File);
    assert_eq!(find("notes.txt").size, Some(5));
    assert!(find("notes.txt").modified.is_some());
    assert!(find(".profile").hidden);
    assert!(!find("notes.txt").hidden);

    // `..` and a trailing slash resolve on the server, so the pane is never
    // left showing a path that is not where the user is.
    let nested = sftp::list(&sftp, "/projects/harbour/../").await.unwrap();
    assert_eq!(nested.path, "/projects");
    assert_eq!(nested.parent.as_deref(), Some("/"));
    assert_eq!(nested.entries.len(), 1);
    assert_eq!(nested.entries[0].name, "harbour");

    let err = sftp::list(&sftp, "/nowhere").await.unwrap_err();
    assert_eq!(err.code(), "FILES_ERROR");

    // One connection, one password: the file channel asked for nothing.
    assert_eq!(asker.secret_questions().len(), 1);

    // ...and the terminal on the same connection still works.
    connected.transport.write(b"still here\r").unwrap();
    read_until(&mut connected.output, "still here").await;
    std::fs::remove_dir_all(&server.sftp_root).ok();
}

#[tokio::test(flavor = "multi_thread")]
async fn the_connection_registry_shares_one_sftp_channel_per_session() {
    let server = start_server().await;
    let connected = client::connect(
        request(server.addr, vec![AuthChoice::Password]),
        ScriptedAsker::trusting(PASSWORD),
        Arc::new(temp_known_hosts()),
        |_, _| {},
    )
    .await
    .expect("the connection should succeed");

    let connections = sftp::Connections::new();
    connections.register("s1".into(), connected.transport.opener());

    let first = connections.sftp("s1").await.unwrap();
    let second = connections.sftp("s1").await.unwrap();
    assert!(
        Arc::ptr_eq(&first, &second),
        "a second ask must reuse the channel, not open another"
    );
    assert!(sftp::list(&first, "/").await.is_ok());

    // A local shell has no entry, and says so rather than hanging.
    let err = connections
        .sftp("local-shell")
        .await
        .err()
        .expect("a local shell has no sftp");
    assert_eq!(err.code(), "SFTP_ERROR");

    connections.remove("s1");
    assert_eq!(
        connections
            .sftp("s1")
            .await
            .err()
            .expect("a removed session has no sftp")
            .code(),
        "SFTP_ERROR"
    );
    std::fs::remove_dir_all(&server.sftp_root).ok();
}

// ---------------------------------------------------------------------------
// Transfer engine tests
// ---------------------------------------------------------------------------

/// An engine whose every emitted snapshot is kept, so a test can assert on
/// the sequence of states as well as the last one.
fn collecting_engine() -> (Arc<Engine>, Arc<Mutex<Vec<Transfer>>>) {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let sink = Arc::clone(&seen);
    let engine = Engine::new(Arc::new(move |transfer: &Transfer| {
        sink.lock().unwrap().push(transfer.clone());
    }));
    (engine, seen)
}

/// Polls until the transfer satisfies `done`, or fails after fifteen seconds.
async fn wait_for(engine: &Engine, id: &str, done: impl Fn(&Transfer) -> bool) -> Transfer {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(15);
    loop {
        let transfer = engine.get(id).expect("the transfer exists");
        if done(&transfer) {
            return transfer;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "timed out waiting; last state {:?} ({:?})",
            transfer.state,
            transfer.error
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

fn finished(transfer: &Transfer) -> bool {
    transfer.state.is_finished()
}

fn temp_local() -> PathBuf {
    let dir = std::env::temp_dir().join(format!("harbour-xfer-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// One fresh connection, with an SFTP channel on it. The transport is leaked
/// on purpose: dropping it would close the connection under the channel, and
/// a test process ends soon enough anyway.
async fn connect_sftp(server: &RunningServer) -> Arc<russh_sftp::client::SftpSession> {
    let connected = client::connect(
        request(server.addr, vec![AuthChoice::Password]),
        ScriptedAsker::trusting(PASSWORD),
        Arc::new(temp_known_hosts()),
        |_, _| {},
    )
    .await
    .expect("the connection should succeed");
    let opener = connected.transport.opener();
    Box::leak(Box::new(connected.transport));
    Arc::new(sftp::open(&opener).await.expect("sftp"))
}

#[tokio::test(flavor = "multi_thread")]
async fn uploads_a_tree_downloads_it_back_and_keeps_time_stamps() {
    let server = start_server().await;
    let sftp = connect_sftp(&server).await;
    let (engine, seen) = collecting_engine();

    let local = temp_local();
    std::fs::create_dir_all(local.join("project").join("src")).unwrap();
    std::fs::write(local.join("project").join("README.md"), "# hello").unwrap();
    std::fs::write(
        local.join("project").join("src").join("main.rs"),
        "fn main() {}",
    )
    .unwrap();
    let stamped = local.join("project").join("README.md");
    std::fs::File::options()
        .write(true)
        .open(&stamped)
        .unwrap()
        .set_modified(std::time::UNIX_EPOCH + Duration::from_secs(1_600_000_000))
        .unwrap();

    let up = engine.enqueue(
        "s1".into(),
        Arc::clone(&sftp),
        Request {
            direction: Direction::Upload,
            source: local.join("project").display().to_string(),
            destination: "/project".into(),
        },
        ConflictPolicy::Ask,
    );
    let up = wait_for(&engine, &up.id, finished).await;

    assert_eq!(up.state, TransferState::Done, "{:?}", up.error);
    assert_eq!(up.files_total, 2);
    assert_eq!(up.files_done, 2);
    assert_eq!(up.bytes_total, 7 + 12);
    assert_eq!(up.bytes_done, up.bytes_total);
    assert_eq!(
        std::fs::read_to_string(server.sftp_root.join("project").join("src").join("main.rs"))
            .unwrap(),
        "fn main() {}"
    );
    let remote_readme =
        std::fs::metadata(server.sftp_root.join("project").join("README.md")).unwrap();
    assert_eq!(
        remote_readme
            .modified()
            .unwrap()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs(),
        1_600_000_000,
        "the source's time stamp travels with the file"
    );

    // The states went queued -> running -> done, and progress was reported.
    let states: Vec<TransferState> = seen
        .lock()
        .unwrap()
        .iter()
        .filter(|t| t.id == up.id)
        .map(|t| t.state)
        .collect();
    assert_eq!(states.first(), Some(&TransferState::Queued));
    assert!(states.contains(&TransferState::Running));
    assert_eq!(states.last(), Some(&TransferState::Done));

    let back = local.join("back");
    let down = engine.enqueue(
        "s1".into(),
        Arc::clone(&sftp),
        Request {
            direction: Direction::Download,
            source: "/project".into(),
            destination: back.display().to_string(),
        },
        ConflictPolicy::Ask,
    );
    let down = wait_for(&engine, &down.id, finished).await;

    assert_eq!(down.state, TransferState::Done, "{:?}", down.error);
    assert_eq!(
        std::fs::read_to_string(back.join("README.md")).unwrap(),
        "# hello"
    );
    assert_eq!(
        std::fs::read_to_string(back.join("src").join("main.rs")).unwrap(),
        "fn main() {}"
    );
    std::fs::remove_dir_all(&local).ok();
    std::fs::remove_dir_all(&server.sftp_root).ok();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_conflict_stops_to_ask_and_the_answer_can_cover_the_rest() {
    let server = start_server().await;
    let sftp = connect_sftp(&server).await;
    let (engine, _) = collecting_engine();

    let local = temp_local();
    std::fs::create_dir_all(local.join("dir")).unwrap();
    std::fs::write(local.join("dir").join("a.txt"), "new a").unwrap();
    std::fs::write(local.join("dir").join("b.txt"), "new b").unwrap();
    std::fs::create_dir_all(server.sftp_root.join("dir")).unwrap();
    std::fs::write(server.sftp_root.join("dir").join("a.txt"), "old a").unwrap();
    std::fs::write(server.sftp_root.join("dir").join("b.txt"), "old b").unwrap();

    let transfer = engine.enqueue(
        "s1".into(),
        Arc::clone(&sftp),
        Request {
            direction: Direction::Upload,
            source: local.join("dir").display().to_string(),
            destination: "/dir".into(),
        },
        ConflictPolicy::Ask,
    );

    let stopped = wait_for(&engine, &transfer.id, |t| {
        t.state == TransferState::Conflict
    })
    .await;
    let conflict = stopped.conflict.expect("the conflict is described");
    assert_eq!(conflict.path, "/dir/a.txt");
    assert_eq!(conflict.source_size, 5);
    assert_eq!(conflict.destination_size, 5);
    assert!(!conflict.resumable);

    // Answer once for everything: the second file must not ask again.
    engine
        .resolve(&transfer.id, Resolution::Skip, true)
        .unwrap();
    let done = wait_for(&engine, &transfer.id, finished).await;

    assert_eq!(done.state, TransferState::Skipped, "{:?}", done.error);
    assert_eq!(done.files_done, 2);
    assert_eq!(
        std::fs::read_to_string(server.sftp_root.join("dir").join("a.txt")).unwrap(),
        "old a"
    );
    assert_eq!(
        std::fs::read_to_string(server.sftp_root.join("dir").join("b.txt")).unwrap(),
        "old b"
    );

    // Resolving something that is not waiting is an error, not a hang.
    assert_eq!(
        engine
            .resolve(&transfer.id, Resolution::Overwrite, false)
            .unwrap_err()
            .code(),
        "TRANSFER_ERROR"
    );
    std::fs::remove_dir_all(&local).ok();
    std::fs::remove_dir_all(&server.sftp_root).ok();
}

#[tokio::test(flavor = "multi_thread")]
async fn resume_continues_a_partial_copy_and_rename_writes_beside() {
    let server = start_server().await;
    let sftp = connect_sftp(&server).await;
    let (engine, _) = collecting_engine();

    let local = temp_local();
    let payload: Vec<u8> = (0..300_000u32).map(|i| (i % 251) as u8).collect();
    std::fs::write(local.join("big.bin"), &payload).unwrap();
    // The remote already holds the first third: a copy that was interrupted.
    std::fs::write(server.sftp_root.join("big.bin"), &payload[..100_000]).unwrap();

    let resumed = engine.enqueue(
        "s1".into(),
        Arc::clone(&sftp),
        Request {
            direction: Direction::Upload,
            source: local.join("big.bin").display().to_string(),
            destination: "/big.bin".into(),
        },
        ConflictPolicy::Resume,
    );
    let resumed = wait_for(&engine, &resumed.id, finished).await;
    assert_eq!(resumed.state, TransferState::Done, "{:?}", resumed.error);
    assert_eq!(
        std::fs::read(server.sftp_root.join("big.bin")).unwrap(),
        payload
    );
    assert_eq!(
        resumed.bytes_done, 300_000,
        "the resumed bytes count as done"
    );

    // Downloading it back with rename leaves the original and writes beside.
    std::fs::write(local.join("big.bin"), b"local copy").unwrap();
    let renamed = engine.enqueue(
        "s1".into(),
        Arc::clone(&sftp),
        Request {
            direction: Direction::Download,
            source: "/big.bin".into(),
            destination: local.join("big.bin").display().to_string(),
        },
        ConflictPolicy::Rename,
    );
    let renamed = wait_for(&engine, &renamed.id, finished).await;
    assert_eq!(renamed.state, TransferState::Done, "{:?}", renamed.error);
    assert_eq!(std::fs::read(local.join("big.bin")).unwrap(), b"local copy");
    assert_eq!(std::fs::read(local.join("big (1).bin")).unwrap(), payload);
    std::fs::remove_dir_all(&local).ok();
    std::fs::remove_dir_all(&server.sftp_root).ok();
}

#[tokio::test(flavor = "multi_thread")]
async fn pause_holds_the_copy_and_cancel_ends_it() {
    let server = start_server().await;
    let sftp = connect_sftp(&server).await;
    let (engine, _) = collecting_engine();

    let local = temp_local();
    std::fs::write(local.join("large.bin"), vec![7u8; 4 * 1024 * 1024]).unwrap();

    let transfer = engine.enqueue(
        "s1".into(),
        Arc::clone(&sftp),
        Request {
            direction: Direction::Upload,
            source: local.join("large.bin").display().to_string(),
            destination: "/large.bin".into(),
        },
        ConflictPolicy::Overwrite,
    );
    engine.pause(&transfer.id).unwrap();

    let paused = wait_for(&engine, &transfer.id, |t| t.state == TransferState::Paused).await;
    assert!(
        paused.bytes_done < paused.bytes_total,
        "it stopped part way"
    );
    let held = paused.bytes_done;
    tokio::time::sleep(Duration::from_millis(150)).await;
    assert_eq!(
        engine.get(&transfer.id).unwrap().bytes_done,
        held,
        "nothing moves while paused"
    );

    engine.cancel(&transfer.id).unwrap();
    let cancelled = wait_for(&engine, &transfer.id, finished).await;
    assert_eq!(cancelled.state, TransferState::Cancelled);

    // A finished transfer can be forgotten; a queued one can be cancelled
    // before it ever runs.
    engine.remove(&transfer.id).unwrap();
    assert_eq!(
        engine.get(&transfer.id).unwrap_err().code(),
        "TRANSFER_NOT_FOUND"
    );
    std::fs::remove_dir_all(&local).ok();
    std::fs::remove_dir_all(&server.sftp_root).ok();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_closing_session_cancels_what_was_queued_on_it() {
    let server = start_server().await;
    let sftp = connect_sftp(&server).await;
    let (engine, _) = collecting_engine();
    let local = temp_local();
    std::fs::write(local.join("x.bin"), vec![1u8; 2 * 1024 * 1024]).unwrap();

    let ids: Vec<String> = (0..4)
        .map(|n| {
            engine
                .enqueue(
                    "s1".into(),
                    Arc::clone(&sftp),
                    Request {
                        direction: Direction::Upload,
                        source: local.join("x.bin").display().to_string(),
                        destination: format!("/x{n}.bin"),
                    },
                    ConflictPolicy::Overwrite,
                )
                .id
        })
        .collect();
    engine.cancel_session("s1");

    for id in &ids {
        let transfer = wait_for(&engine, id, finished).await;
        assert!(
            matches!(
                transfer.state,
                TransferState::Cancelled | TransferState::Done
            ),
            "{:?}",
            transfer.state
        );
    }
    let cancelled = engine
        .list()
        .iter()
        .filter(|t| t.state == TransferState::Cancelled)
        .count();
    assert!(
        cancelled >= 2,
        "at least the ones beyond the two slots never ran"
    );
    assert_eq!(engine.clear_finished(), 4);
    assert!(engine.list().is_empty());
    std::fs::remove_dir_all(&local).ok();
    std::fs::remove_dir_all(&server.sftp_root).ok();
}

// ---------------------------------------------------------------------------
// Open in editor
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread")]
async fn an_edited_file_is_uploaded_on_save_and_cleaned_up_on_close() {
    let server = start_server().await;
    let sftp = connect_sftp(&server).await;
    std::fs::write(server.sftp_root.join("notes.txt"), "before").unwrap();

    let seen = Arc::new(Mutex::new(Vec::new()));
    let sink = Arc::clone(&seen);
    let editor = Editor::new(Arc::new(move |info: &harbour_lib::edit::EditInfo| {
        sink.lock().unwrap().push(info.clone());
    }));

    let launched = Arc::new(Mutex::new(None));
    let record = Arc::clone(&launched);
    let info = editor
        .open("s1".into(), Arc::clone(&sftp), "/notes.txt", move |path| {
            *record.lock().unwrap() = Some(path.to_path_buf());
            Ok(())
        })
        .await
        .expect("open");

    let local = launched
        .lock()
        .unwrap()
        .clone()
        .expect("the editor was launched");
    assert_eq!(local.display().to_string(), info.local_path);
    assert_eq!(std::fs::read_to_string(&local).unwrap(), "before");

    // The user saves.
    std::fs::write(&local, "after").unwrap();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(15);
    loop {
        let uploads = editor.list().first().map(|e| e.uploads).unwrap_or(0);
        if uploads >= 1 {
            break;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "the save was never uploaded"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert_eq!(
        std::fs::read_to_string(server.sftp_root.join("notes.txt")).unwrap(),
        "after"
    );
    assert!(seen
        .lock()
        .unwrap()
        .iter()
        .any(|e| e.uploads == 1 && e.error.is_none()));

    editor.close(&info.id).unwrap();
    assert!(!local.exists(), "the working copy is removed");
    assert!(editor.list().is_empty());
    assert_eq!(editor.close(&info.id).unwrap_err().code(), "EDIT_ERROR");
    std::fs::remove_dir_all(&server.sftp_root).ok();
}
