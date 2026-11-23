// SPDX-License-Identifier: AGPL-3.0-or-later

//! `rest_is_ip_address()`: a dotted quad, or what `Requests\Ipv6::check_ipv6()`
//! takes as an IPv6 address.

/// Whether `ip` is an IPv4 or IPv6 address as PHP reads it; `None` where
/// PHP's `str_repeat()` would throw, as for more groups than eight.
pub(super) fn is_ip_address(ip: &[u8]) -> Option<bool> {
    if is_ipv4(ip) {
        return Some(true);
    }
    let Ok(ip) = std::str::from_utf8(ip) else {
        return Some(false);
    };
    check_ipv6(ip)
}

/// `/^(?:(?:25[0-5]|2[0-4][0-9]|[01]?[0-9][0-9]?)\.){3}(...)$/`: four
/// numbers of at most three digits and at most 255, where `$` also matches
/// before a last new line.
fn is_ipv4(ip: &[u8]) -> bool {
    let ip = ip.strip_suffix(b"\n").unwrap_or(ip);
    let octets: Vec<&[u8]> = ip.split(|&b| b == b'.').collect();
    octets.len() == 4
        && octets.iter().all(|octet| {
            (1..=3).contains(&octet.len())
                && octet.iter().all(u8::is_ascii_digit)
                && octet
                    .iter()
                    .fold(0u32, |n, &b| n * 10 + u32::from(b - b'0'))
                    <= 255
        })
}

/// `Ipv6::uncompress()`: the `::` of an address filled with zero groups.
fn uncompress(ip: &str) -> Option<String> {
    if ip.matches("::").count() != 1 {
        return Some(ip.to_string());
    }
    let (first, second) = ip.split_once("::").unwrap_or_default();
    let groups = |part: &str| match part {
        "" => -1,
        part => i64::try_from(part.matches(':').count()).unwrap_or_default(),
    };
    let (c1, mut c2) = (groups(first), groups(second));
    if second.contains('.') {
        c2 += 1;
    }
    let repeat = |unit: &str, times: i64| usize::try_from(times).ok().map(|n| unit.repeat(n));
    let fill = match (c1, c2) {
        (-1, -1) => return Some("0:0:0:0:0:0:0:0".to_string()),
        (-1, _) => repeat("0:", 7 - c2)?,
        (_, -1) => repeat(":0", 7 - c1)?,
        _ => format!(":{}", repeat("0:", 6 - c2 - c1)?),
    };
    Some(ip.replace("::", &fill))
}

/// `Ipv6::check_ipv6()`.
fn check_ipv6(ip: &str) -> Option<bool> {
    let ip = uncompress(ip)?;
    let at = ip.rfind(':').unwrap_or_default();
    let (v6, v4) = match ip.contains('.') {
        true => ip.split_at(at),
        false => (ip.as_str(), ""),
    };
    let v4 = v4.strip_prefix(':').unwrap_or(v4);
    let groups: Vec<&str> = v6.split(':').collect();
    let quad: Vec<&str> = v4.split('.').collect();
    let shaped = (groups.len() == 8 && quad.len() == 1) || (groups.len() == 6 && quad.len() == 4);
    let hex = |group: &&str| {
        (1..=4).contains(&group.len()) && group.bytes().all(|b| b.is_ascii_hexdigit())
    };
    // `(string) (int) $part === $part`, from 0 to 255.
    let number = |part: &&str| {
        let canonical =
            part.bytes().all(|b| b.is_ascii_digit()) && (part.len() == 1 || !part.starts_with('0'));
        canonical && part.parse::<u32>().is_ok_and(|n| n <= 255)
    };
    Some(shaped && groups.iter().all(hex) && (quad.len() == 1 || quad.iter().all(number)))
}
