//! Local port forwarding: a TCP port on this machine carried to a host the
//! remote can reach, over the SSH connection a terminal already has.
//!
//! This is `ssh -L`. Harbour listens on a local address; each connection that
//! arrives opens a `direct-tcpip` channel on the session's connection to the
//! forward's target and copies bytes both ways. Nothing new authenticates and
//! nothing new is trusted: a forward can only reach what its session can, and
//! it dies with the session.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use tokio::net::TcpListener;

use crate::error::{AppError, AppResult};
use crate::session::SessionId;
use crate::ssh::transport::ChannelOpener;

pub type ForwardId = String;

/// What to forward: listen here, deliver there.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ForwardSpec {
    /// Where to listen. `127.0.0.1` keeps the forward to this machine;
    /// `0.0.0.0` exposes it on the network, which the UI warns about.
    pub bind_address: String,
    /// `0` asks the OS for a free port, reported back in the info.
    pub local_port: u16,
    /// The target, resolved on the remote side - so `localhost` means the
    /// remote's own localhost, which is the usual point of a forward.
    pub host: String,
    pub port: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ForwardState {
    Listening,
    Closed,
    Failed,
}

/// A forward as the frontend sees it, sent whole on every change.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ForwardInfo {
    pub id: ForwardId,
    pub session_id: SessionId,
    pub bind_address: String,
    /// The port actually bound, which differs from the request when it asked
    /// for `0`.
    pub local_port: u16,
    pub host: String,
    pub port: u16,
    pub state: ForwardState,
    /// How many connections have been accepted over the life of the forward.
    pub connections: u32,
    pub error: Option<String>,
}

pub type Emitter = Arc<dyn Fn(&ForwardInfo) + Send + Sync>;

struct Forward {
    info: ForwardInfo,
    /// Aborting this stops the accept loop and drops the listener.
    task: tauri::async_runtime::JoinHandle<()>,
}

pub struct Forwards {
    inner: Mutex<HashMap<ForwardId, Forward>>,
    emit: Emitter,
}

impl Forwards {
    pub fn new(emit: Emitter) -> Arc<Self> {
        Arc::new(Self {
            inner: Mutex::new(HashMap::new()),
            emit,
        })
    }

    /// Binds the listener and starts forwarding. The bind happens here, before
    /// returning, so "address already in use" is an error the caller sees
    /// rather than a silent failure inside the accept loop.
    pub async fn open_local(
        self: &Arc<Self>,
        session_id: SessionId,
        opener: ChannelOpener,
        spec: ForwardSpec,
    ) -> AppResult<ForwardInfo> {
        let bind = format!("{}:{}", spec.bind_address, spec.local_port);
        let listener = TcpListener::bind(&bind)
            .await
            .map_err(|err| AppError::Forward(format!("could not listen on {bind}: {err}")))?;
        let local_port = listener
            .local_addr()
            .map(|addr| addr.port())
            .unwrap_or(spec.local_port);

        let id = uuid::Uuid::new_v4().to_string();
        let connections = Arc::new(AtomicU32::new(0));
        let info = ForwardInfo {
            id: id.clone(),
            session_id,
            bind_address: spec.bind_address.clone(),
            local_port,
            host: spec.host.clone(),
            port: spec.port,
            state: ForwardState::Listening,
            connections: 0,
            error: None,
        };

        let engine = Arc::clone(self);
        let accept_id = id.clone();
        let accept_conns = Arc::clone(&connections);
        let target = (spec.host.clone(), spec.port);
        let task = tauri::async_runtime::spawn(async move {
            engine
                .accept_loop(accept_id, listener, opener, target, accept_conns)
                .await;
        });

        self.inner.lock().insert(
            id,
            Forward {
                info: info.clone(),
                task,
            },
        );
        (self.emit)(&info);
        Ok(info)
    }

    async fn accept_loop(
        self: Arc<Self>,
        id: ForwardId,
        listener: TcpListener,
        opener: ChannelOpener,
        target: (String, u16),
        connections: Arc<AtomicU32>,
    ) {
        loop {
            let (mut socket, _peer) = match listener.accept().await {
                Ok(accepted) => accepted,
                // The listener closed (the forward was removed), or the OS
                // refused; either way there is nothing more to accept.
                Err(err) => {
                    tracing::debug!(forward = %id, error = %err, "forward accept ended");
                    break;
                }
            };
            let count = connections.fetch_add(1, Ordering::AcqRel) + 1;
            self.update(&id, |info| info.connections = count);

            let opener = opener.clone();
            let (host, port) = target.clone();
            let forward_id = id.clone();
            let engine = Arc::clone(&self);
            tauri::async_runtime::spawn(async move {
                match opener.open_forward(&host, port).await {
                    Ok(channel) => {
                        let mut stream = channel.into_stream();
                        // Ends when either side closes; a forwarded connection
                        // closing is normal and says nothing about the forward.
                        let _ = tokio::io::copy_bidirectional(&mut socket, &mut stream).await;
                    }
                    Err(err) => {
                        // The connection could not be opened - the remote
                        // refused, or the session went away. The forward stays
                        // up; this one connection failed.
                        engine.update(&forward_id, |info| {
                            info.error = Some(err.to_string());
                        });
                    }
                }
            });
        }
    }

    fn update(&self, id: &str, change: impl FnOnce(&mut ForwardInfo)) {
        let info = {
            let mut inner = self.inner.lock();
            let Some(forward) = inner.get_mut(id) else {
                return;
            };
            change(&mut forward.info);
            forward.info.clone()
        };
        (self.emit)(&info);
    }

    pub fn list(&self) -> Vec<ForwardInfo> {
        self.inner
            .lock()
            .values()
            .map(|forward| forward.info.clone())
            .collect()
    }

    /// Stops one forward: the listener closes and open connections drop.
    pub fn close(&self, id: &str) -> AppResult<()> {
        let forward = self
            .inner
            .lock()
            .remove(id)
            .ok_or_else(|| AppError::Forward(format!("no forward {id}")))?;
        forward.task.abort();
        let mut info = forward.info;
        info.state = ForwardState::Closed;
        (self.emit)(&info);
        Ok(())
    }

    /// The session is gone; so is every forward that rode it.
    pub fn close_session(&self, session_id: &str) {
        let ids: Vec<ForwardId> = self
            .inner
            .lock()
            .values()
            .filter(|forward| forward.info.session_id == session_id)
            .map(|forward| forward.info.id.clone())
            .collect();
        for id in ids {
            let _ = self.close(&id);
        }
    }
}

/// Parses `[bind:]localPort:host:port`, as `ssh -L` takes it, so a forward can
/// be typed in one line. IPv6 literals in brackets are accepted for the bind.
pub fn parse_spec(text: &str) -> AppResult<ForwardSpec> {
    let bad = || AppError::Forward(format!("`{text}` is not localPort:host:port"));
    let (bind_address, rest) = match text.strip_prefix('[') {
        Some(after) => {
            let close = after.find(']').ok_or_else(bad)?;
            let bind = after[..close].to_string();
            let rest = after[close + 1..].strip_prefix(':').ok_or_else(bad)?;
            (bind, rest.to_string())
        }
        None => (String::new(), text.to_string()),
    };

    let parts: Vec<&str> = rest.rsplitn(3, ':').collect();
    if parts.len() != 3 {
        return Err(bad());
    }
    // rsplitn yields them reversed: [port, host, localPort-or-bind:localPort].
    let port: u16 = parts[0].parse().map_err(|_| bad())?;
    let host = parts[1].to_string();
    let head = parts[2];

    let (bind_address, local) = if !bind_address.is_empty() {
        (bind_address, head.to_string())
    } else if let Some((bind, local)) = head.rsplit_once(':') {
        (bind.to_string(), local.to_string())
    } else {
        ("127.0.0.1".to_string(), head.to_string())
    };
    let local_port: u16 = local.parse().map_err(|_| bad())?;
    if host.is_empty() {
        return Err(bad());
    }

    Ok(ForwardSpec {
        bind_address: if bind_address.is_empty() {
            "127.0.0.1".to_string()
        } else {
            bind_address
        },
        local_port,
        host,
        port,
    })
}

/// Whether a bind address exposes the forward beyond this machine, which the
/// UI flags: a local forward on `0.0.0.0` is a hole in the firewall.
pub fn is_public_bind(address: &str) -> bool {
    match address.parse::<std::net::IpAddr>() {
        Ok(ip) => !ip.is_loopback(),
        // A name that is not an address: treat anything but the usual local
        // names as public, erring towards warning.
        Err(_) => !matches!(address, "localhost" | ""),
    }
}

/// A convenience for the UI: a free loopback port, so "any port" can be shown
/// as a concrete number before the forward is created.
pub fn any_local_port() -> Option<u16> {
    std::net::TcpListener::bind(("127.0.0.1", 0))
        .ok()
        .and_then(|listener| listener.local_addr().ok())
        .map(|addr: SocketAddr| addr.port())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_the_ssh_dash_l_forms() {
        assert_eq!(
            parse_spec("8080:localhost:80").unwrap(),
            ForwardSpec {
                bind_address: "127.0.0.1".into(),
                local_port: 8080,
                host: "localhost".into(),
                port: 80,
            }
        );
        assert_eq!(
            parse_spec("0.0.0.0:5432:db.internal:5432").unwrap(),
            ForwardSpec {
                bind_address: "0.0.0.0".into(),
                local_port: 5432,
                host: "db.internal".into(),
                port: 5432,
            }
        );
        // A bracketed IPv6 bind.
        assert_eq!(
            parse_spec("[::1]:9000:localhost:9000")
                .unwrap()
                .bind_address,
            "::1"
        );
    }

    #[test]
    fn rejects_what_is_not_a_forward() {
        assert!(parse_spec("80").is_err());
        assert!(parse_spec("host:80").is_err());
        assert!(parse_spec("8080:host:notaport").is_err());
        assert!(parse_spec("8080::80").is_err());
    }

    #[test]
    fn a_public_bind_is_recognised() {
        assert!(!is_public_bind("127.0.0.1"));
        assert!(!is_public_bind("::1"));
        assert!(!is_public_bind("localhost"));
        assert!(is_public_bind("0.0.0.0"));
        assert!(is_public_bind("192.168.1.10"));
    }
}
