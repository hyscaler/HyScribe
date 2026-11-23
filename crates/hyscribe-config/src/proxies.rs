// SPDX-License-Identifier: AGPL-3.0-or-later

//! `TRUSTED_PROXIES`: the peers `X-Forwarded-For` is believed from.

use std::net::IpAddr;

/// The most entries a list may have.
pub const MAX_PROXIES: usize = 256;

/// The longest list text read, in bytes.
const MAX_TEXT: usize = 16 * 1024;

/// One address or range: `192.0.2.1`, `10.0.0.0/8`, `fd00::/8`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cidr {
    network: u128,
    mask: u128,
    v6: bool,
}

impl Cidr {
    /// An address, or an address and a prefix length, as one entry reads;
    /// `None` for anything else.
    pub fn parse(entry: &str) -> Option<Cidr> {
        let (address, length) = match entry.split_once('/') {
            Some((address, length)) => (address, Some(length)),
            None => (entry, None),
        };
        let ip: IpAddr = address.parse().ok()?;
        let (bits, v6) = match ip {
            IpAddr::V4(_) => (32, false),
            IpAddr::V6(_) => (128, true),
        };
        let length = match length {
            // Digits only: no sign, no space.
            Some(length) if length.bytes().all(|b| b.is_ascii_digit()) => length
                .parse::<u32>()
                .ok()
                .filter(|length| *length <= bits)?,
            Some(_) => return None,
            None => bits,
        };
        // An IPv4-mapped entry with a prefix inside the mapped block names
        // IPv4 addresses, as a peer's mapped address is read as IPv4.
        let (ip, v6, length) = match ip.to_canonical() {
            IpAddr::V4(v4) if v6 && length >= 96 => (IpAddr::V4(v4), false, length - 96),
            _ => (ip, v6, length),
        };
        let value = match ip {
            IpAddr::V4(ip) => u128::from(u32::from(ip)),
            IpAddr::V6(ip) => u128::from(ip),
        };
        let all = if v6 { u128::MAX } else { u128::from(u32::MAX) };
        // A shift by the full width has no bits left.
        let mask = all & !all.checked_shr(length).unwrap_or(0);
        Some(Cidr {
            network: value & mask,
            mask,
            v6,
        })
    }

    /// Whether `ip` is in the range; an IPv4-mapped IPv6 address counts as
    /// its IPv4 address.
    pub fn contains(&self, ip: IpAddr) -> bool {
        let matches = |ip: IpAddr| {
            let (value, v6) = match ip {
                IpAddr::V4(ip) => (u128::from(u32::from(ip)), false),
                IpAddr::V6(ip) => (u128::from(ip), true),
            };
            v6 == self.v6 && value & self.mask == self.network
        };
        // A mapped address also stays an IPv6 address, for an IPv6 range
        // wider than the mapped block.
        matches(ip.to_canonical()) || matches(ip)
    }
}

/// The ranges of a comma separated list; `None` where it is too long, has
/// too many entries or one that is no address or range. Empty entries are
/// skipped.
pub fn parse_list(text: &str) -> Option<Vec<Cidr>> {
    if text.len() > MAX_TEXT {
        return None;
    }
    let mut list = Vec::new();
    for entry in text.split(',').map(str::trim).filter(|e| !e.is_empty()) {
        if list.len() == MAX_PROXIES {
            return None;
        }
        list.push(Cidr::parse(entry)?);
    }
    Some(list)
}

/// The list used where `TRUSTED_PROXIES` is not set: loopback.
pub fn default_list() -> Vec<Cidr> {
    parse_list("127.0.0.1,::1").unwrap_or_default()
}

#[cfg(test)]
mod tests;
