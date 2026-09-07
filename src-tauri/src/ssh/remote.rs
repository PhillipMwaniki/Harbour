//! The routing table for remote (`-R`) forwards on one connection.
//!
//! A remote forward is the mirror of a local one. Harbour asks the server to
//! listen on a port (a `tcpip-forward` global request); every connection the
//! server accepts there it pushes back as a `forwarded-tcpip` channel, which
//! the connection handler in [`client`](crate::ssh::client) routes through this
//! table to a target *this* machine can reach.
//!
//! The table is keyed by the remote port, because that - `connected_port` - is
//! all a `forwarded-tcpip` channel names. It is shared between the handler,
//! which reads it to place each incoming channel, and the forward engine, which
//! registers a target when a forward opens and forgets it when the forward
//! closes. A channel that arrives for a port with no entry is rejected: the
//! forward is gone, or was never ours.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

use parking_lot::Mutex;

/// Where a `forwarded-tcpip` channel for a given remote port is delivered, plus
/// a hook to report each new connection so the forward's card shows activity.
struct Route {
    host: String,
    port: u16,
    connections: AtomicU32,
    /// Called with the running connection count each time a channel arrives.
    /// An `Arc` so it can be cloned out and invoked without holding the lock.
    notify: Arc<dyn Fn(u32) + Send + Sync>,
}

/// The remote forwards active on one connection, by the port the server listens
/// on. Empty for a session with no remote forwards, which is the common case
/// and costs nothing.
#[derive(Default)]
pub struct RemoteForwards {
    routes: Mutex<HashMap<u32, Route>>,
}

// `Route` holds a closure, which is not `Debug`; the ports it routes are what a
// log would want anyway.
impl std::fmt::Debug for RemoteForwards {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let ports: Vec<u32> = self.routes.lock().keys().copied().collect();
        f.debug_struct("RemoteForwards")
            .field("ports", &ports)
            .finish()
    }
}

impl RemoteForwards {
    /// Starts routing channels that arrive on `remote_port` to `host:port`.
    /// Registering before the `tcpip-forward` request is sent means a
    /// connection the server forwards the instant it accepts still lands.
    pub fn register(
        &self,
        remote_port: u32,
        host: String,
        port: u16,
        notify: Arc<dyn Fn(u32) + Send + Sync>,
    ) {
        self.routes.lock().insert(
            remote_port,
            Route {
                host,
                port,
                connections: AtomicU32::new(0),
                notify,
            },
        );
    }

    /// Moves a route to a new port. Only the server-allocated case (a forward
    /// asked for port 0) needs it: the route is registered under 0, then rehomed
    /// once the server names the port it chose.
    pub fn rekey(&self, from: u32, to: u32) {
        if from == to {
            return;
        }
        let mut routes = self.routes.lock();
        if let Some(route) = routes.remove(&from) {
            routes.insert(to, route);
        }
    }

    /// Stops routing `remote_port`. Channels that arrive after this are
    /// rejected, which is what the server should see once the forward is gone.
    pub fn unregister(&self, remote_port: u32) {
        self.routes.lock().remove(&remote_port);
    }

    /// The local target for a channel that arrived on `remote_port`, if one is
    /// registered. Bumps and reports the connection count as it does, so the
    /// lookup and the accounting cannot drift apart. Returns `None` when no
    /// forward owns the port, telling the handler to reject the channel.
    pub fn accept(&self, remote_port: u32) -> Option<(String, u16)> {
        let (host, port, count, notify) = {
            let routes = self.routes.lock();
            let route = routes.get(&remote_port)?;
            let count = route.connections.fetch_add(1, Ordering::AcqRel) + 1;
            (
                route.host.clone(),
                route.port,
                count,
                Arc::clone(&route.notify),
            )
        };
        notify(count);
        Some((host, port))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicU32;

    fn silent() -> Arc<dyn Fn(u32) + Send + Sync> {
        Arc::new(|_| {})
    }

    #[test]
    fn routes_a_registered_port_and_rejects_an_unknown_one() {
        let forwards = RemoteForwards::default();
        forwards.register(8080, "127.0.0.1".into(), 3000, silent());

        assert_eq!(forwards.accept(8080), Some(("127.0.0.1".to_string(), 3000)));
        // Nothing listens on 9090.
        assert_eq!(forwards.accept(9090), None);
    }

    #[test]
    fn a_closed_forward_stops_routing() {
        let forwards = RemoteForwards::default();
        forwards.register(8080, "localhost".into(), 22, silent());
        forwards.unregister(8080);
        assert_eq!(forwards.accept(8080), None);
    }

    #[test]
    fn rekey_rehomes_a_server_allocated_port() {
        let forwards = RemoteForwards::default();
        forwards.register(0, "localhost".into(), 5432, silent());
        forwards.rekey(0, 54321);
        assert_eq!(forwards.accept(0), None);
        assert_eq!(
            forwards.accept(54321),
            Some(("localhost".to_string(), 5432))
        );
    }

    #[test]
    fn each_accepted_channel_reports_a_rising_count() {
        let seen = Arc::new(AtomicU32::new(0));
        let sink = Arc::clone(&seen);
        let forwards = RemoteForwards::default();
        forwards.register(
            8080,
            "localhost".into(),
            80,
            Arc::new(move |count| sink.store(count, Ordering::Release)),
        );

        forwards.accept(8080);
        assert_eq!(seen.load(Ordering::Acquire), 1);
        forwards.accept(8080);
        assert_eq!(seen.load(Ordering::Acquire), 2);
    }
}
