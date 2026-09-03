//! End-to-end tests for the SSH client, against a real SSH server.
//!
//! The server is a `russh` server started in-process on a loopback port, so
//! these run everywhere CI does, with no fixtures and no network. What they
//! cover is the part unit tests cannot reach: the handshake, host key
//! verification against a store, authentication, the pty and shell requests,
//! and bytes travelling in both directions over a live channel.

use std::collections::VecDeque;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{mpsc as std_mpsc, Arc, Mutex};
use std::time::Duration;

use harbour_lib::error::AppResult;
use harbour_lib::session::{ExitReason, Transport};
use harbour_lib::ssh::client::{self, ConnectRequest};
use harbour_lib::ssh::known_hosts::KnownHosts;
use harbour_lib::ssh::{
    Asker, AuthChoice, HostKeyAnswer, HostKeyQuestion, HostKeyStatus, SecretAnswer, SecretKind,
    SecretQuestion, SshTarget,
};
use russh::keys::{Algorithm, PrivateKey};
use russh::server::{Auth, Msg, Server as _, Session};
use russh::{Channel, ChannelId, MethodKind, MethodSet};
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
        _channel: Channel<Msg>,
        reply: russh::server::ChannelOpenHandle,
        _session: &mut Session,
    ) -> Result<(), Self::Error> {
        reply.accept().await;
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

    tokio::spawn(async move {
        let mut server = TestServer { seen: seen_tx };
        let _ = server.run_on_socket(config, &listener).await;
    });

    RunningServer { addr, seen }
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
