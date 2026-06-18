//! Discovery. `scan()` is the one entry point: enumerate every listening
//! socket from `/proc/net/*`, then walk the ownership chain for each. The two
//! halves live in focused submodules — [`sockets`] reads the kernel tables,
//! [`owner`] turns a socket inode into pid -> process -> unit -> package — so
//! each can be reasoned about and tested on its own.

pub mod owner;
pub mod sockets;

use crate::error::PortmanError;
use crate::model::Listener;

/// Enumerate all listening sockets and resolve each one's ownership chain.
///
/// Listeners are returned sorted for stable output and stable diffs: by
/// exposure (most-public first), then port, then proto. The chain resolution is
/// best-effort throughout — see [`owner`] — so a non-root run still returns
/// every socket, just with thinner [`crate::model::Owner`] data.
pub fn scan() -> Result<Vec<Listener>, PortmanError> {
    let raw = sockets::listening()?;
    let inode_map = owner::InodeMap::build();

    let mut listeners: Vec<Listener> = raw
        .into_iter()
        .map(|s| Listener {
            owner: owner::resolve(s.inode, &inode_map),
            proto: s.proto,
            addr: s.addr,
            port: s.port,
            exposure: s.exposure,
        })
        .collect();

    listeners.sort_by(sort_key);
    listeners.dedup_by(|a, b| a.key() == b.key());
    Ok(listeners)
}

/// Stable ordering: most-exposed first (PUBLIC > iface > local), then port,
/// then proto. Puts the listeners an operator most cares about at the top.
fn sort_key(a: &Listener, b: &Listener) -> std::cmp::Ordering {
    use crate::model::Exposure::*;
    let rank = |e: crate::model::Exposure| match e {
        AllInterfaces => 0,
        Interface => 1,
        Loopback => 2,
    };
    rank(a.exposure)
        .cmp(&rank(b.exposure))
        .then(a.port.cmp(&b.port))
        .then(a.proto.cmp(&b.proto))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Exposure, Owner, Proto};

    fn l(exposure: Exposure, port: u16, proto: Proto) -> Listener {
        Listener {
            proto,
            addr: "x".into(),
            port,
            exposure,
            owner: Owner::unknown(),
        }
    }

    #[test]
    fn sort_puts_public_then_port_first() {
        let mut v = [
            l(Exposure::Loopback, 631, Proto::Tcp),
            l(Exposure::AllInterfaces, 443, Proto::Tcp),
            l(Exposure::AllInterfaces, 22, Proto::Tcp),
            l(Exposure::Interface, 53, Proto::Udp),
        ];
        v.sort_by(sort_key);
        assert_eq!(v[0].port, 22); // public, lowest port
        assert_eq!(v[1].port, 443); // public
        assert_eq!(v[2].port, 53); // iface
        assert_eq!(v[3].port, 631); // loopback last
    }
}
