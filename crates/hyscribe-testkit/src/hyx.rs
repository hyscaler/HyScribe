// SPDX-License-Identifier: AGPL-3.0-or-later

//! `.hyx` packages for tests: zip archives as common tools write them.

/// A zip archive of `entries`, each a name, its bytes and whether it is
/// deflated rather than stored; a name ending in `/` is a folder.
pub fn zip(entries: &[(&str, &[u8], bool)]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut central = Vec::new();
    for &(name, data, deflate) in entries {
        let folder = name.ends_with('/');
        let packed = if deflate {
            miniz_oxide::deflate::compress_to_vec(data, 6)
        } else {
            data.to_vec()
        };
        let method: u16 = if deflate { 8 } else { 0 };
        let mode: u32 = if folder { 0o040_755 } else { 0o100_644 };
        let sizes = [
            crc32fast::hash(data),
            u32::try_from(packed.len()).unwrap_or(u32::MAX),
            u32::try_from(data.len()).unwrap_or(u32::MAX),
        ];
        let name_len = u16::try_from(name.len()).unwrap_or(u16::MAX);
        let offset = u32::try_from(out.len()).unwrap_or(u32::MAX);
        let mut common = Vec::new();
        common.extend(20u16.to_le_bytes());
        common.extend(0x0800u16.to_le_bytes());
        common.extend(method.to_le_bytes());
        common.extend([0; 4]);
        sizes.iter().for_each(|n| common.extend(n.to_le_bytes()));
        common.extend(name_len.to_le_bytes());
        common.extend(0u16.to_le_bytes());
        out.extend(0x0403_4b50u32.to_le_bytes());
        out.extend(&common);
        out.extend(name.as_bytes());
        out.extend(packed);
        central.extend(0x0201_4b50u32.to_le_bytes());
        central.extend((3u16 << 8 | 20).to_le_bytes());
        central.extend(&common);
        central.extend([0; 6]);
        central.extend((mode << 16).to_le_bytes());
        central.extend(offset.to_le_bytes());
        central.extend(name.as_bytes());
    }
    let count = u16::try_from(entries.len()).unwrap_or(u16::MAX);
    let start = u32::try_from(out.len()).unwrap_or(u32::MAX);
    let size = u32::try_from(central.len()).unwrap_or(u32::MAX);
    out.extend(central);
    out.extend(0x0605_4b50u32.to_le_bytes());
    out.extend([0; 4]);
    out.extend(count.to_le_bytes());
    out.extend(count.to_le_bytes());
    out.extend(size.to_le_bytes());
    out.extend(start.to_le_bytes());
    out.extend(0u16.to_le_bytes());
    out
}
