//! Establishing an SSH connection and getting a shell on it.
//!
//! The shape of this module follows OpenSSH's: verify the host key, walk a
//! list of authentication methods until one succeeds, then open one channel,
//! ask for a pty on it, and start a shell. What is Harbour-specific is that
//! every decision a human has to make is delegated to an [`Asker`] rather than
//! read from a terminal, and that a refused host key is an error with a code
//! the UI can act on.

use std::borrow::Cow;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use parking_lot::Mutex;
use russh::client::{AuthResult, Config, Handle, Handler, KeyboardInteractiveAuthResponse, Msg};
use russh::keys::agent::AgentIdentity;
use russh::keys::ssh_key::Algorithm;
use russh::keys::{load_secret_key, PrivateKeyWithHashAlg, PublicKeyOrCertificate};
use russh::{Channel, ChannelMsg, MethodKind, MethodSet, SshId};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::sync::mpsc;

use crate::error::{AppError, AppResult};
use crate::session::ExitReason;
use crate::ssh::known_hosts::{fingerprint, KnownHosts, Verdict};
use crate::ssh::transport::{self, SshTransport};
use crate::ssh::{
    agent, Asker, AuthChoice, HostKeyAnswer, HostKeyQuestion, HostKeyStatus, SecretAnswer,
    SecretKind, SecretQuestion, SshTarget,
};

/// An object-safe view of an [`Asker`].
///
/// A jump chain is a list of hops, and each hop answers for a different saved
/// host - its own host key, its own password. That means one asker per hop,
/// held in a `Vec`, which a generic `A: Asker` cannot be. This is the same
/// interface, boxed, so the chain can carry a different asker at every hop
/// while [`Asker`] stays ergonomic to implement.
pub trait DynAsker: Send + Sync {
    fn host_key<'a>(
        &'a self,
        question: HostKeyQuestion,
    ) -> Pin<Box<dyn Future<Output = AppResult<HostKeyAnswer>> + Send + 'a>>;

    fn secret<'a>(
        &'a self,
        question: SecretQuestion,
    ) -> Pin<Box<dyn Future<Output = AppResult<SecretAnswer>> + Send + 'a>>;
}

impl<A: Asker> DynAsker for A {
    fn host_key<'a>(
        &'a self,
        question: HostKeyQuestion,
    ) -> Pin<Box<dyn Future<Output = AppResult<HostKeyAnswer>> + Send + 'a>> {
        Box::pin(Asker::host_key(self, question))
    }

    fn secret<'a>(
        &'a self,
        question: SecretQuestion,
    ) -> Pin<Box<dyn Future<Output = AppResult<SecretAnswer>> + Send + 'a>> {
        Box::pin(Asker::secret(self, question))
    }
}

/// A stream a russh client can run over: a TCP socket to the first host, then
/// a `direct-tcpip` channel through each jump. Boxed so the connect loop can
/// carry either without caring which.
trait Stream: AsyncRead + AsyncWrite + Unpin + Send {}
impl<T: AsyncRead + AsyncWrite + Unpin + Send + ?Sized> Stream for T {}

/// One connection in a chain: where to reach it, how to authenticate, and who
/// answers its prompts.
pub struct Endpoint {
    pub target: SshTarget,
    pub methods: Vec<AuthChoice>,
    pub asker: Arc<dyn DynAsker>,
}

/// The terminal type advertised to the remote. It is what xterm.js implements,
/// and what the local pty path already claims.
const TERM: &str = "xterm-256color";

/// Sent if the connection goes quiet, so a dropped link is noticed in seconds
/// rather than whenever the user next types. Three unanswered probes end it.
const KEEPALIVE: Duration = Duration::from_secs(30);

pub struct ConnectRequest {
    pub target: SshTarget,
    /// Authentication methods to try, in order.
    pub methods: Vec<AuthChoice>,
    pub cols: u16,
    pub rows: u16,
}

/// A connected, authenticated session with a live shell on it.
#[derive(Debug)]
pub struct Connected {
    pub transport: SshTransport,
    pub output: mpsc::Receiver<Vec<u8>>,
    /// `SHA256:...` of the host key that was accepted.
    pub fingerprint: String,
    /// Which method authenticated, for the log and the UI.
    pub method: &'static str,
}

/// Connects, authenticates, and starts a remote shell.
///
/// `on_exit` fires when the shell ends or the connection drops, mirroring the
/// local pty path so the session manager treats both the same way.
pub async fn connect<F>(
    request: ConnectRequest,
    asker: Arc<dyn DynAsker>,
    known_hosts: Arc<KnownHosts>,
    on_exit: F,
) -> AppResult<Connected>
where
    F: FnOnce(ExitReason, Option<u32>) + Send + 'static,
{
    let dest = Endpoint {
        target: request.target,
        methods: request.methods,
        asker,
    };
    connect_chain(
        Vec::new(),
        dest,
        known_hosts,
        request.cols,
        request.rows,
        on_exit,
    )
    .await
}

/// Connects to `dest`, tunnelling through `jumps` in order first.
///
/// Each jump is a full SSH connection - its own host key check, its own
/// authentication - over which a `direct-tcpip` channel is opened to the next
/// hop; the next connection runs over that channel. One password per hop, one
/// host key decision per hop, exactly as `ssh -J` behaves. The jump
/// connections are kept alive for the life of the session and torn down with
/// it: closing the terminal closes the whole chain.
pub async fn connect_chain<F>(
    jumps: Vec<Endpoint>,
    dest: Endpoint,
    known_hosts: Arc<KnownHosts>,
    cols: u16,
    rows: u16,
    on_exit: F,
) -> AppResult<Connected>
where
    F: FnOnce(ExitReason, Option<u32>) + Send + 'static,
{
    let established = establish(&jumps, &dest, &known_hosts).await?;
    let Established {
        session,
        hops,
        fingerprint,
        method,
    } = established;

    let mut channel = session
        .channel_open_session()
        .await
        .map_err(|err| AppError::SshChannel(err.to_string()))?;

    channel
        .request_pty(
            true,
            TERM,
            u32::from(cols.max(1)),
            u32::from(rows.max(1)),
            0,
            0,
            &[],
        )
        .await
        .map_err(|err| AppError::SshChannel(err.to_string()))?;
    let mut pending = wait_for_reply(&mut channel, "pty").await?;

    channel
        .request_shell(true)
        .await
        .map_err(|err| AppError::SshChannel(err.to_string()))?;
    pending.extend(wait_for_reply(&mut channel, "shell").await?);

    // The jump handles ride along with the session and drop when it ends.
    let running = transport::start(session, channel, pending, Box::new(hops), on_exit);

    tracing::info!(
        host = %dest.target.host,
        port = dest.target.port,
        jumps = jumps.len(),
        method,
        "ssh session established"
    );

    Ok(Connected {
        transport: running.transport,
        output: running.output,
        fingerprint,
        method,
    })
}

/// A connected, authenticated session, before a shell or a command is put on
/// it. Shared by the interactive path ([`connect_chain`]) and the one-shot
/// command path ([`run_command`]).
struct Established {
    session: Handle<ClientHandler>,
    /// The jump connections, kept alive so the chain stays up; dropping one
    /// closes everything nested inside it.
    hops: Vec<Handle<ClientHandler>>,
    fingerprint: String,
    method: &'static str,
}

/// Connects to `dest` through `jumps`, checking each host key and
/// authenticating each hop, and returns the live session.
async fn establish(
    jumps: &[Endpoint],
    dest: &Endpoint,
    known_hosts: &Arc<KnownHosts>,
) -> AppResult<Established> {
    let cfg = Arc::new(config());

    // The socket to the first host in the chain. Made here, rather than
    // through `russh::client::connect`, so "no route to host" stays
    // distinguishable from an SSH-level failure.
    let first = jumps.first().unwrap_or(dest);
    let tcp = tokio::net::TcpStream::connect((first.target.host.as_str(), first.target.port))
        .await
        .map_err(|err| AppError::SshConnect {
            host: first.target.host.clone(),
            port: first.target.port,
            reason: err.to_string(),
        })?;
    let _ = tcp.set_nodelay(true);
    let mut stream: Box<dyn Stream> = Box::new(tcp);

    // Walk the jumps, tunnelling one hop deeper each time.
    let mut hops: Vec<Handle<ClientHandler>> = Vec::with_capacity(jumps.len());
    for (index, hop) in jumps.iter().enumerate() {
        let handler =
            ClientHandler::new(&hop.target, Arc::clone(known_hosts), Arc::clone(&hop.asker));
        let mut session = russh::client::connect_stream(Arc::clone(&cfg), stream, handler).await?;
        authenticate(&mut session, &hop.target, &hop.methods, hop.asker.as_ref()).await?;

        let next = jumps.get(index + 1).unwrap_or(dest);
        let channel = session
            .channel_open_direct_tcpip(
                next.target.host.clone(),
                u32::from(next.target.port),
                "127.0.0.1",
                0,
            )
            .await
            .map_err(|err| AppError::SshConnect {
                host: next.target.host.clone(),
                port: next.target.port,
                reason: format!("via {}: {err}", hop.target.label()),
            })?;
        stream = Box::new(channel.into_stream());
        hops.push(session);
    }

    let accepted = Arc::new(Mutex::new(None));
    let handler = ClientHandler::new(
        &dest.target,
        Arc::clone(known_hosts),
        Arc::clone(&dest.asker),
    )
    .recording(Arc::clone(&accepted));
    let mut session = russh::client::connect_stream(cfg, stream, handler).await?;
    let method = authenticate(
        &mut session,
        &dest.target,
        &dest.methods,
        dest.asker.as_ref(),
    )
    .await?;

    let fingerprint = accepted
        .lock()
        .clone()
        .unwrap_or_else(|| "unknown".to_string());

    Ok(Established {
        session,
        hops,
        fingerprint,
        method,
    })
}

/// What running one command on a host produced.
#[derive(Debug, Clone)]
pub struct CommandOutcome {
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    /// The command's exit status, when the server reported one.
    pub exit_code: Option<u32>,
}

/// Connects to `dest` (through `jumps`), runs one command, and disconnects.
///
/// This is the fleet runner's unit of work. It opens no pty and no shell: it
/// `exec`s the command, collects stdout and stderr to the end, and lets the
/// connection close as the session drops. Nothing here is interactive - a host
/// that would need a prompt (an untrusted key, a password with nowhere to come
/// from) fails, which is what an unattended run across many hosts wants.
pub async fn run_command(
    jumps: Vec<Endpoint>,
    dest: Endpoint,
    known_hosts: Arc<KnownHosts>,
    command: &str,
) -> AppResult<CommandOutcome> {
    let established = establish(&jumps, &dest, &known_hosts).await?;
    let mut channel = established
        .session
        .channel_open_session()
        .await
        .map_err(|err| AppError::SshChannel(err.to_string()))?;

    channel
        .exec(true, command)
        .await
        .map_err(|err| AppError::SshChannel(err.to_string()))?;

    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let mut exit_code = None;
    while let Some(message) = channel.wait().await {
        match message {
            ChannelMsg::Data { data } => stdout.extend_from_slice(&data),
            // Extended data type 1 is stderr; other types are not used here.
            ChannelMsg::ExtendedData { data, ext: 1 } => stderr.extend_from_slice(&data),
            ChannelMsg::ExitStatus { exit_status } => exit_code = Some(exit_status),
            ChannelMsg::Eof | ChannelMsg::Close => break,
            _ => {}
        }
    }
    // `established` (and its jump hops) drop here, closing the connection.

    Ok(CommandOutcome {
        stdout,
        stderr,
        exit_code,
    })
}

fn config() -> Config {
    Config {
        client_id: SshId::Standard(Cow::Borrowed(concat!(
            "SSH-2.0-Harbour_",
            env!("CARGO_PKG_VERSION")
        ))),
        keepalive_interval: Some(KEEPALIVE),
        keepalive_max: 3,
        nodelay: true,
        ..Config::default()
    }
}

/// Waits for the reply to a channel request, keeping any output that arrives
/// alongside it.
///
/// A server may start writing before we have read its `SUCCESS`, and the first
/// thing it writes is the shell's opening prompt. Dropping it on the floor
/// would leave the user looking at a blank terminal - the same class of bug as
/// the milestone 1 pty sizing fix - so it is buffered and replayed instead.
pub(crate) async fn wait_for_reply(channel: &mut Channel<Msg>, what: &str) -> AppResult<Vec<u8>> {
    let mut buffered = Vec::new();
    while let Some(message) = channel.wait().await {
        match message {
            ChannelMsg::Success => return Ok(buffered),
            ChannelMsg::Failure => {
                return Err(AppError::SshChannel(format!(
                    "the server refused the {what} request"
                )))
            }
            ChannelMsg::Data { data } | ChannelMsg::ExtendedData { data, .. } => {
                buffered.extend_from_slice(&data)
            }
            ChannelMsg::Eof | ChannelMsg::Close => break,
            _ => {}
        }
    }
    Err(AppError::SshChannel(format!(
        "the channel closed while waiting for the {what} reply"
    )))
}

/// What one authentication attempt came to.
enum Outcome {
    Success,
    /// Rejected; the server says these methods are still open.
    Failure(MethodSet),
    /// The user dismissed a prompt. Not a failure - stop, quietly.
    Cancelled,
}

/// Tries each method in turn, skipping any the server will not entertain.
async fn authenticate(
    session: &mut Handle<ClientHandler>,
    target: &SshTarget,
    methods: &[AuthChoice],
    asker: &dyn DynAsker,
) -> AppResult<&'static str> {
    // `none` is both a real method - some hosts accept it - and the standard
    // way to find out what else the server will take.
    let mut remaining = match session.authenticate_none(&target.user).await? {
        AuthResult::Success => return Ok("none"),
        AuthResult::Failure {
            remaining_methods, ..
        } => remaining_methods,
    };

    let mut tried: Vec<String> = Vec::new();
    let mut refused: Vec<&'static str> = Vec::new();

    for choice in methods {
        if !remaining.contains(&method_kind(choice)) {
            refused.push(choice.describe());
            continue;
        }
        tried.push(choice.describe().to_string());

        let outcome = match choice {
            // No agent, an agent that hung up, or one holding nothing: the
            // method failed, and the next one is tried - exactly what `ssh`
            // does. Only a failure *talking to the server* stops the attempt.
            AuthChoice::Agent => match authenticate_with_agent(session, target).await {
                Ok(outcome) => outcome,
                Err(AppError::SshAgent(reason)) => {
                    tracing::debug!(reason = %reason, "ssh agent unusable; trying the next method");
                    if let Some(last) = tried.last_mut() {
                        *last = format!("agent ({reason})");
                    }
                    Outcome::Failure(remaining.clone())
                }
                Err(err) => return Err(err),
            },
            AuthChoice::Key { path } => authenticate_with_key(session, target, path, asker).await?,
            AuthChoice::Password => authenticate_with_password(session, target, asker).await?,
            AuthChoice::KeyboardInteractive => {
                authenticate_interactively(session, target, asker).await?
            }
        };

        match outcome {
            Outcome::Success => return Ok(choice.describe()),
            Outcome::Failure(still_open) => remaining = still_open,
            Outcome::Cancelled => {
                return Err(AppError::SshAuth {
                    host: target.host.clone(),
                    user: target.user.clone(),
                    reason: "cancelled".into(),
                })
            }
        }
    }

    Err(AppError::SshAuth {
        host: target.host.clone(),
        user: target.user.clone(),
        reason: describe_failure(&tried, &refused, &remaining),
    })
}

fn method_kind(choice: &AuthChoice) -> MethodKind {
    match choice {
        AuthChoice::Agent | AuthChoice::Key { .. } => MethodKind::PublicKey,
        AuthChoice::Password => MethodKind::Password,
        AuthChoice::KeyboardInteractive => MethodKind::KeyboardInteractive,
    }
}

/// The message a user gets when nothing worked. "Authentication failed" alone
/// is useless when the real problem is that the server never offered the
/// method they configured.
fn describe_failure(tried: &[String], refused: &[&str], remaining: &MethodSet) -> String {
    let mut parts = Vec::new();
    if !tried.is_empty() {
        parts.push(format!("tried {}", tried.join(", ")));
    }
    if !refused.is_empty() {
        parts.push(format!("{} not offered by the server", refused.join(", ")));
    }
    if !remaining.is_empty() {
        let offered: Vec<&str> = remaining.iter().map(Into::into).collect();
        parts.push(format!("server accepts {}", offered.join(", ")));
    }
    if parts.is_empty() {
        "no authentication method was available".into()
    } else {
        parts.join("; ")
    }
}

/// Offers every identity the agent holds, the way `ssh -A` would.
async fn authenticate_with_agent(
    session: &mut Handle<ClientHandler>,
    target: &SshTarget,
) -> AppResult<Outcome> {
    let mut agent = agent::connect().await?;
    let identities = agent
        .request_identities()
        .await
        .map_err(|err| AppError::SshAgent(err.to_string()))?;

    if identities.is_empty() {
        return Err(AppError::SshAgent("the agent holds no identities".into()));
    }

    let rsa_hash = session.best_supported_rsa_hash().await?.flatten();
    let mut last = MethodSet::empty();

    for identity in identities {
        // Certificates need the CA plumbing that milestone 9 adds; a keyring
        // holding both should still get to try its plain keys.
        let AgentIdentity::PublicKey { key, comment } = identity else {
            continue;
        };
        let hash_alg = rsa_hash_for(&key.algorithm(), rsa_hash);

        match session
            .authenticate_publickey_with(&target.user, key, hash_alg, &mut agent)
            .await
            .map_err(|err| AppError::SshAgent(err.to_string()))?
        {
            AuthResult::Success => {
                tracing::debug!(identity = %comment, "agent identity accepted");
                return Ok(Outcome::Success);
            }
            AuthResult::Failure {
                remaining_methods, ..
            } => last = remaining_methods,
        }
    }

    Ok(Outcome::Failure(last))
}

/// Loads a key file, asking for the passphrase only if the file turns out to
/// need one.
async fn authenticate_with_key(
    session: &mut Handle<ClientHandler>,
    target: &SshTarget,
    path: &str,
    asker: &dyn DynAsker,
) -> AppResult<Outcome> {
    let key = match load_secret_key(path, None) {
        Ok(key) => key,
        Err(russh::keys::Error::KeyIsEncrypted) => {
            let answer = asker
                .secret(SecretQuestion {
                    host: target.host.clone(),
                    user: target.user.clone(),
                    kind: SecretKind::Passphrase,
                    label: format!("Passphrase for {path}"),
                    instruction: String::new(),
                    echo: false,
                    can_remember: false,
                })
                .await?;
            let Some(passphrase) = answer.secret else {
                return Ok(Outcome::Cancelled);
            };
            load_secret_key(path, Some(&passphrase)).map_err(|err| AppError::SshKeyLoad {
                path: path.to_string(),
                reason: err.to_string(),
            })?
        }
        Err(err) => {
            return Err(AppError::SshKeyLoad {
                path: path.to_string(),
                reason: err.to_string(),
            })
        }
    };

    let rsa_hash = session.best_supported_rsa_hash().await?.flatten();
    let hash_alg = rsa_hash_for(&key.algorithm(), rsa_hash);
    let key = PrivateKeyWithHashAlg::new(Arc::new(key), hash_alg);

    Ok(
        match session.authenticate_publickey(&target.user, key).await? {
            AuthResult::Success => Outcome::Success,
            AuthResult::Failure {
                remaining_methods, ..
            } => Outcome::Failure(remaining_methods),
        },
    )
}

async fn authenticate_with_password(
    session: &mut Handle<ClientHandler>,
    target: &SshTarget,
    asker: &dyn DynAsker,
) -> AppResult<Outcome> {
    let answer = asker
        .secret(SecretQuestion {
            host: target.host.clone(),
            user: target.user.clone(),
            kind: SecretKind::Password,
            label: format!("Password for {}", target.label()),
            instruction: String::new(),
            echo: false,
            can_remember: false,
        })
        .await?;
    let Some(password) = answer.secret else {
        return Ok(Outcome::Cancelled);
    };

    Ok(
        match session
            .authenticate_password(&target.user, password)
            .await?
        {
            AuthResult::Success => Outcome::Success,
            AuthResult::Failure {
                remaining_methods, ..
            } => Outcome::Failure(remaining_methods),
        },
    )
}

/// `keyboard-interactive`, where the server words the questions. This is how
/// password login is actually implemented on a good many hosts, and it is the
/// only route for one-time-code prompts.
async fn authenticate_interactively(
    session: &mut Handle<ClientHandler>,
    target: &SshTarget,
    asker: &dyn DynAsker,
) -> AppResult<Outcome> {
    let mut response = session
        .authenticate_keyboard_interactive_start(&target.user, None)
        .await?;

    loop {
        match response {
            KeyboardInteractiveAuthResponse::Success => return Ok(Outcome::Success),
            KeyboardInteractiveAuthResponse::Failure {
                remaining_methods, ..
            } => return Ok(Outcome::Failure(remaining_methods)),
            KeyboardInteractiveAuthResponse::InfoRequest {
                name,
                instructions,
                prompts,
            } => {
                let instruction = [name.trim(), instructions.trim()]
                    .iter()
                    .filter(|part| !part.is_empty())
                    .cloned()
                    .collect::<Vec<_>>()
                    .join("\n");

                let mut answers = Vec::with_capacity(prompts.len());
                for prompt in prompts {
                    let answer = asker
                        .secret(SecretQuestion {
                            host: target.host.clone(),
                            user: target.user.clone(),
                            kind: SecretKind::Challenge,
                            label: prompt.prompt.trim().to_string(),
                            instruction: instruction.clone(),
                            echo: prompt.echo,
                            can_remember: false,
                        })
                        .await?;
                    let Some(secret) = answer.secret else {
                        return Ok(Outcome::Cancelled);
                    };
                    answers.push(secret);
                }

                response = session
                    .authenticate_keyboard_interactive_respond(answers)
                    .await?;
            }
        }
    }
}

/// RSA signatures need an explicit hash: `ssh-rsa` with SHA-1 is rejected by
/// modern servers, so the best hash the server named is used instead. Every
/// other algorithm carries its hash in the algorithm name.
fn rsa_hash_for(
    algorithm: &Algorithm,
    best: Option<russh::keys::HashAlg>,
) -> Option<russh::keys::HashAlg> {
    match algorithm {
        Algorithm::Rsa { .. } => best,
        _ => None,
    }
}

/// The connection-side half of the host key policy in `docs/security.md`.
struct ClientHandler {
    host: String,
    port: u16,
    known_hosts: Arc<KnownHosts>,
    asker: Arc<dyn DynAsker>,
    /// Set to the accepted host key's fingerprint. Only the destination's is
    /// read; a jump's handler leaves it `None`.
    accepted: Arc<Mutex<Option<String>>>,
}

impl ClientHandler {
    fn new(target: &SshTarget, known_hosts: Arc<KnownHosts>, asker: Arc<dyn DynAsker>) -> Self {
        Self {
            host: target.host.clone(),
            port: target.port,
            known_hosts,
            asker,
            accepted: Arc::new(Mutex::new(None)),
        }
    }

    /// Records the accepted fingerprint into a shared slot the caller reads.
    fn recording(mut self, accepted: Arc<Mutex<Option<String>>>) -> Self {
        self.accepted = accepted;
        self
    }
}

impl Handler for ClientHandler {
    type Error = AppError;

    async fn check_server_key(&mut self, offered: &PublicKeyOrCertificate) -> AppResult<bool> {
        let PublicKeyOrCertificate::PublicKey { key, .. } = offered else {
            return Err(AppError::SshHostKeyRejected {
                host: self.host.clone(),
                reason:
                    "the server presented a host certificate, which Harbour cannot validate yet"
                        .into(),
            });
        };

        let print = fingerprint(key);
        let (status, stored) = match self.known_hosts.verify(&self.host, self.port, key) {
            Verdict::Trusted => {
                *self.accepted.lock() = Some(print);
                return Ok(true);
            }
            Verdict::Revoked => {
                return Err(AppError::SshHostKeyRejected {
                    host: self.host.clone(),
                    reason: "this key is marked @revoked in known_hosts".into(),
                })
            }
            Verdict::Unknown { other } => (HostKeyStatus::Unknown, other),
            Verdict::Changed { stored } => (HostKeyStatus::Changed, stored),
        };

        let answer = self
            .asker
            .host_key(HostKeyQuestion {
                host: self.host.clone(),
                port: self.port,
                status,
                algorithm: key.algorithm().to_string(),
                fingerprint: print.clone(),
                stored,
            })
            .await?;

        if !answer.accept {
            return Err(AppError::SshHostKeyRejected {
                host: self.host.clone(),
                reason: match status {
                    HostKeyStatus::Changed => {
                        "the offered key did not match the one on file".into()
                    }
                    HostKeyStatus::Unknown => "the key was not trusted".into(),
                },
            });
        }

        if answer.remember {
            // Failing to record the key is not a reason to refuse a connection
            // the user just approved; they will simply be asked again.
            if let Err(err) = self.known_hosts.learn(&self.host, self.port, key) {
                tracing::warn!(error = %err, path = %self.known_hosts.write_path().display(), "could not record the host key");
            }
        }

        *self.accepted.lock() = Some(print);
        Ok(true)
    }

    /// Servers use the banner for legal notices and MOTDs. It belongs on the
    /// terminal eventually; for now it is logged, never discarded silently.
    async fn auth_banner(
        &mut self,
        banner: &str,
        _session: &mut russh::client::Session,
    ) -> AppResult<()> {
        tracing::info!(host = %self.host, banner = %banner.trim(), "server banner");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn methods(kinds: &[MethodKind]) -> MethodSet {
        let mut set = MethodSet::empty();
        for kind in kinds {
            set.push(*kind);
        }
        set
    }

    #[test]
    fn every_choice_maps_to_the_method_the_server_names() {
        assert_eq!(method_kind(&AuthChoice::Agent), MethodKind::PublicKey);
        assert_eq!(
            method_kind(&AuthChoice::Key {
                path: "id_ed25519".into()
            }),
            MethodKind::PublicKey
        );
        assert_eq!(method_kind(&AuthChoice::Password), MethodKind::Password);
        assert_eq!(
            method_kind(&AuthChoice::KeyboardInteractive),
            MethodKind::KeyboardInteractive
        );
    }

    /// The common confusion is a host with `PasswordAuthentication no`. Saying
    /// so is the difference between a two-second fix and a lost afternoon.
    #[test]
    fn a_method_the_server_never_offered_is_named_as_such() {
        let message = describe_failure(
            &["publickey".to_string()],
            &["password"],
            &methods(&[MethodKind::PublicKey]),
        );
        assert!(message.contains("tried publickey"));
        assert!(message.contains("password not offered"));
        assert!(message.contains("server accepts publickey"));
    }

    #[test]
    fn exhausting_every_method_still_says_something_useful() {
        let message = describe_failure(&[], &[], &MethodSet::empty());
        assert_eq!(message, "no authentication method was available");
    }

    #[test]
    fn only_rsa_keys_are_given_an_explicit_hash() {
        let best = Some(russh::keys::HashAlg::Sha512);
        assert_eq!(
            rsa_hash_for(&Algorithm::Rsa { hash: None }, best),
            Some(russh::keys::HashAlg::Sha512)
        );
        assert_eq!(rsa_hash_for(&Algorithm::Ed25519, best), None);
    }
}
