//! Parse listening sockets straight out of `/proc/net/{tcp,tcp6,udp,udp6}`.
//!
//! No `ss`/`netstat` dependency: those files are the same source `ss` reads.
//! Each line has the kernel's hex-encoded local address, a connection state,
//! and the socket inode we later match to an owning pid. The parsing of one
//! line is a pure function so its hex/endianness handling is fully testable
//! without touching the filesystem.

use std::fs;
use std::net::{Ipv4Addr, Ipv6Addr};

use crate::error::PortmanError;
use crate::model::{Exposure, Proto};

/// A listening socket as read from the kernel tables, before ownership lookup.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawSocket {
    pub proto: Proto,
    pub addr: String,
    pub port: u16,
    pub exposure: Exposure,
    /// Socket inode — the join key into `/proc/*/fd` for owner resolution.
    pub inode: u64,
}

/// The TCP "LISTEN" state in `/proc/net/tcp` (st column, hex `0A`).
const TCP_LISTEN: &str = "0A";

/// Read every `/proc/net/*` table and return the listening sockets.
///
/// A missing/unreadable *individual* table (e.g. no IPv6) is skipped, not
/// fatal. Only a total inability to read the TCP table is an error — that means
/// `/proc/net` itself is gone, and portman has nothing to report.
pub fn listening() -> Result<Vec<RawSocket>, PortmanError> {
    let tcp = fs::read_to_string("/proc/net/tcp")
        .map_err(|source| PortmanError::NoProc { source })?;

    let mut out = parse_table(&tcp, Proto::Tcp);
    for (path, proto) in [
        ("/proc/net/tcp6", Proto::Tcp6),
        ("/proc/net/udp", Proto::Udp),
        ("/proc/net/udp6", Proto::Udp6),
    ] {
        if let Ok(text) = fs::read_to_string(path) {
            out.extend(parse_table(&text, proto));
        }
    }
    Ok(out)
}

/// Parse one whole table, keeping only listening sockets. TCP keeps only the
/// LISTEN state; UDP has no LISTEN state, so a UDP socket bound to a wildcard
/// remote (`0.0.0.0:0`) is treated as listening — the same rule `ss` applies.
fn parse_table(text: &str, proto: Proto) -> Vec<RawSocket> {
    text.lines()
        .skip(1) // header row
        .filter_map(|line| parse_line(line, proto))
        .collect()
}

/// Parse a single table row into a [`RawSocket`], or `None` if it isn't a
/// listener (or the line is malformed). Pure: all the hex/endianness logic
/// lives here so it can be tested against known kernel output.
fn parse_line(line: &str, proto: Proto) -> Option<RawSocket> {
    let mut cols = line.split_whitespace();
    // Columns: sl  local_address  rem_address  st ... inode (col 9).
    let _sl = cols.next()?;
    let local = cols.next()?;
    let remote = cols.next()?;
    let state = cols.next()?;

    if proto.is_tcp() {
        if state != TCP_LISTEN {
            return None;
        }
    } else {
        // UDP "listening" = no connected peer (remote address is all-zero).
        if !is_unbound_remote(remote) {
            return None;
        }
    }

    // inode is column index 9 (0-based) on the line.
    let inode: u64 = line.split_whitespace().nth(9)?.parse().ok()?;

    let (addr, port) = parse_addr(local, proto)?;
    let exposure = Exposure::classify(&addr);
    Some(RawSocket {
        proto,
        addr,
        port,
        exposure,
        inode,
    })
}

/// Whether a `rem_address` field encodes "no peer" (all-zero address, port 0),
/// which for UDP means the socket is a listener rather than a connected flow.
fn is_unbound_remote(remote: &str) -> bool {
    match remote.rsplit_once(':') {
        Some((host, port)) => {
            port.eq_ignore_ascii_case("0000") && host.bytes().all(|b| b == b'0')
        }
        None => false,
    }
}

/// Decode the `HEXADDR:HEXPORT` local-address field. IPv4 is 8 hex chars in
/// host byte order (little-endian on the wire here); IPv6 is 32 hex chars in
/// four little-endian 32-bit words. Returns the text address and the port.
fn parse_addr(field: &str, proto: Proto) -> Option<(String, u16)> {
    let (host_hex, port_hex) = field.rsplit_once(':')?;
    let port = u16::from_str_radix(port_hex, 16).ok()?;

    let addr = match proto {
        Proto::Tcp | Proto::Udp => decode_v4(host_hex)?,
        Proto::Tcp6 | Proto::Udp6 => decode_v6(host_hex)?,
    };
    Some((addr, port))
}

/// Decode 8 hex chars (little-endian u32) into a dotted IPv4 string.
fn decode_v4(hex: &str) -> Option<String> {
    if hex.len() != 8 {
        return None;
    }
    let raw = u32::from_str_radix(hex, 16).ok()?;
    // Kernel writes the address little-endian; swap to host order for octets.
    Some(Ipv4Addr::from(raw.to_be()).to_string())
}

/// Decode 32 hex chars (four little-endian u32 words) into an IPv6 string.
fn decode_v6(hex: &str) -> Option<String> {
    if hex.len() != 32 {
        return None;
    }
    let mut octets = [0u8; 16];
    for word in 0..4 {
        let chunk = &hex[word * 8..word * 8 + 8];
        let raw = u32::from_str_radix(chunk, 16).ok()?;
        // The kernel writes each 32-bit word little-endian; laying the word out
        // in its native (little-endian) byte order reproduces the four address
        // bytes for this word in order. (`to_be()` then `to_be_bytes()` would
        // double-swap; `to_le_bytes()` is the single correct transform here.)
        octets[word * 4..word * 4 + 4].copy_from_slice(&raw.to_le_bytes());
    }
    Some(Ipv6Addr::from(octets).to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decode_v4_wildcard_and_loopback() {
        // 0.0.0.0
        assert_eq!(decode_v4("00000000").unwrap(), "0.0.0.0");
        // 127.0.0.1 stored little-endian as 0100007F
        assert_eq!(decode_v4("0100007F").unwrap(), "127.0.0.1");
        // 192.168.1.10 -> 0A01A8C0 little-endian
        assert_eq!(decode_v4("0A01A8C0").unwrap(), "192.168.1.10");
    }

    #[test]
    fn decode_v6_unspecified_and_loopback() {
        assert_eq!(
            decode_v6("00000000000000000000000000000000").unwrap(),
            "::"
        );
        // ::1 — loopback is the last byte set, little-endian per word.
        assert_eq!(
            decode_v6("00000000000000000000000001000000").unwrap(),
            "::1"
        );
    }

    #[test]
    fn parse_line_keeps_only_tcp_listen() {
        // A real-ish /proc/net/tcp LISTEN row for 0.0.0.0:22 (port 0x0016=22).
        let listen = "   0: 00000000:0016 00000000:0000 0A 00000000:00000000 00:00000000 00000000     0        0 12345 1 0000000000000000 100 0 0 10 0";
        let s = parse_line(listen, Proto::Tcp).expect("a listener");
        assert_eq!(s.addr, "0.0.0.0");
        assert_eq!(s.port, 22);
        assert_eq!(s.inode, 12345);
        assert_eq!(s.exposure, Exposure::AllInterfaces);

        // ESTABLISHED (st 01) is not a listener.
        let established = listen.replacen(" 0A ", " 01 ", 1);
        assert!(parse_line(&established, Proto::Tcp).is_none());
    }

    #[test]
    fn parse_line_udp_listener_has_zero_remote() {
        // UDP bound to 0.0.0.0:53 with no peer (state column is irrelevant).
        let row = "   1: 00000000:0035 00000000:0000 07 00000000:00000000 00:00000000 00000000     0        0 67890 2 0000000000000000 0";
        let s = parse_line(row, Proto::Udp).expect("a udp listener");
        assert_eq!(s.port, 53);
        assert_eq!(s.inode, 67890);

        // A UDP socket with a connected peer is not a listener.
        let connected = row.replacen("00000000:0000 07", "0100007F:1F90 01", 1);
        assert!(parse_line(&connected, Proto::Udp).is_none());
    }

    #[test]
    fn parse_table_skips_header() {
        let table = "  sl  local_address rem_address   st ...\n   0: 0100007F:1538 00000000:0000 0A 00000000:00000000 00:00000000 00000000     0        0 999 1 0";
        let rows = parse_table(table, Proto::Tcp);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].addr, "127.0.0.1");
        assert_eq!(rows[0].exposure, Exposure::Loopback);
    }
}
