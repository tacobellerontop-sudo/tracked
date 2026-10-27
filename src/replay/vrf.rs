//! Inspector for VALORANT's downloaded replay files (`.vrf`).
//!
//! VALORANT can download a match replay to disk (by default under
//! `%LOCALAPPDATA%\VALORANT\Saved\Demos`). The format is undocumented;
//! community write-ups describe it as Unreal Engine's local-file replay
//! container with a Riot-specific marker in front. This module only *reads
//! files on disk* (never the game process) and reports their structure so a
//! position parser can be designed against real files. It prints no match
//! contents beyond short hex prefixes.

use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

/// Unreal's local-file replay magic (`FLocalFileNetworkReplayStreamer`).
pub const UE_REPLAY_MAGIC: u32 = 0x1CA2_E27F;

/// Where VALORANT keeps downloaded replays, if the folder exists.
pub fn demos_dir() -> Option<PathBuf> {
    let base = std::env::var_os("LOCALAPPDATA")?;
    let dir = PathBuf::from(base).join("VALORANT").join("Saved").join("Demos");
    dir.is_dir().then_some(dir)
}

/// `.vrf` files in `dir`, newest first.
pub fn list(dir: &Path) -> Vec<PathBuf> {
    let mut files: Vec<(std::time::SystemTime, PathBuf)> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x.eq_ignore_ascii_case("vrf")))
        .filter_map(|p| Some((p.metadata().ok()?.modified().ok()?, p)))
        .collect();
    files.sort_by_key(|f| std::cmp::Reverse(f.0));
    files.into_iter().map(|(_, p)| p).collect()
}

#[derive(Debug, Default, Clone, PartialEq)]
pub struct UeHeader {
    pub offset: u64,
    pub file_version: u32,
    pub length_ms: u32,
    pub network_version: u32,
    pub changelist: u32,
    pub friendly_name: String,
    pub is_live: bool,
    pub compressed: Option<bool>,
    pub encrypted: Option<bool>,
}

#[derive(Debug, Default, Clone, PartialEq)]
pub struct ChunkInfo {
    pub offset: u64,
    pub kind: u32,
    pub size: u32,
    /// First bytes of the payload, hex.
    pub head: String,
    /// Shannon entropy of the first 4 KiB, bits per byte (≈8 = compressed
    /// or encrypted, lower = raw structured data).
    pub entropy: f32,
}

#[derive(Debug, Default, Clone, PartialEq)]
pub struct Report {
    pub size: u64,
    /// First 64 bytes of the file, hex (the Riot marker lives here).
    pub prefix: String,
    pub header: Option<UeHeader>,
    pub chunks: Vec<ChunkInfo>,
    /// Why the walk stopped early, if it did.
    pub note: Option<String>,
}

pub fn chunk_name(kind: u32) -> &'static str {
    match kind {
        0 => "Header",
        1 => "ReplayData",
        2 => "Checkpoint",
        3 => "Event",
        _ => "Unknown",
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect::<Vec<_>>().join(" ")
}

pub fn entropy(bytes: &[u8]) -> f32 {
    if bytes.is_empty() {
        return 0.0;
    }
    let mut counts = [0u32; 256];
    for b in bytes {
        counts[*b as usize] += 1;
    }
    let n = bytes.len() as f32;
    counts
        .iter()
        .filter(|c| **c > 0)
        .map(|c| {
            let p = *c as f32 / n;
            -p * p.log2()
        })
        .sum()
}

struct Cursor<'a, R: Read + Seek> {
    r: &'a mut R,
}

impl<R: Read + Seek> Cursor<'_, R> {
    fn u32(&mut self) -> std::io::Result<u32> {
        let mut b = [0u8; 4];
        self.r.read_exact(&mut b)?;
        Ok(u32::from_le_bytes(b))
    }
    fn i64(&mut self) -> std::io::Result<i64> {
        let mut b = [0u8; 8];
        self.r.read_exact(&mut b)?;
        Ok(i64::from_le_bytes(b))
    }
    /// Unreal `FString`: i32 length (negative = UTF-16), including the NUL.
    fn fstring(&mut self) -> std::io::Result<String> {
        let len = self.u32()? as i32;
        if len == 0 {
            return Ok(String::new());
        }
        if !(-4096..=4096).contains(&len) {
            return Err(std::io::Error::other(format!("implausible string length {len}")));
        }
        if len > 0 {
            let mut b = vec![0u8; len as usize];
            self.r.read_exact(&mut b)?;
            Ok(String::from_utf8_lossy(&b).trim_end_matches('\0').to_string())
        } else {
            let mut b = vec![0u8; (-len) as usize * 2];
            self.r.read_exact(&mut b)?;
            let units: Vec<u16> = b.chunks(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
            Ok(String::from_utf16_lossy(&units).trim_end_matches('\0').to_string())
        }
    }
}

/// Find Unreal's replay magic within the first 64 KiB (after any Riot marker).
fn find_magic(buf: &[u8]) -> Option<usize> {
    let magic = UE_REPLAY_MAGIC.to_le_bytes();
    buf.windows(4).position(|w| w == magic)
}

/// Inspect a replay: the prefix, the Unreal header if found, and every chunk
/// (type, size, a short hex head and an entropy estimate).
pub fn inspect<R: Read + Seek>(r: &mut R) -> std::io::Result<Report> {
    let size = r.seek(SeekFrom::End(0))?;
    r.seek(SeekFrom::Start(0))?;
    let mut first = vec![0u8; size.min(64 * 1024) as usize];
    r.read_exact(&mut first)?;
    let mut report = Report {
        size,
        prefix: hex(&first[..first.len().min(64)]),
        ..Default::default()
    };
    let Some(at) = find_magic(&first) else {
        report.note = Some("no Unreal replay magic in the first 64 KiB".into());
        return Ok(report);
    };
    r.seek(SeekFrom::Start(at as u64 + 4))?;
    let mut c = Cursor { r };
    let mut h = UeHeader {
        offset: at as u64,
        ..Default::default()
    };
    let parsed: std::io::Result<()> = (|| {
        h.file_version = c.u32()?;
        h.length_ms = c.u32()?;
        h.network_version = c.u32()?;
        h.changelist = c.u32()?;
        h.friendly_name = c.fstring()?;
        h.is_live = c.u32()? != 0;
        // Fields added in later Unreal file versions.
        if h.file_version >= 3 {
            let _timestamp = c.i64()?;
        }
        if h.file_version >= 2 {
            h.compressed = Some(c.u32()? != 0);
        }
        if h.file_version >= 6 {
            h.encrypted = Some(c.u32()? != 0);
            let key_len = c.u32()?;
            if key_len > 1024 {
                return Err(std::io::Error::other(format!("implausible key length {key_len}")));
            }
            c.r.seek(SeekFrom::Current(key_len as i64))?;
        }
        Ok(())
    })();
    report.header = Some(h);
    if let Err(e) = parsed {
        report.note = Some(format!("header didn't parse as a known Unreal version: {e}"));
        return Ok(report);
    }
    // Chunks: u32 type, i32 size, payload.
    loop {
        let offset = c.r.stream_position()?;
        if offset + 8 > size {
            break;
        }
        let kind = c.u32()?;
        let len = c.u32()?;
        if kind > 16 || offset + 8 + len as u64 > size {
            report.note = Some(format!(
                "stopped at offset {offset}: chunk type {kind} / size {len} doesn't fit the Unreal layout"
            ));
            break;
        }
        let mut sample = vec![0u8; (len as usize).min(4096)];
        c.r.read_exact(&mut sample)?;
        report.chunks.push(ChunkInfo {
            offset,
            kind,
            size: len,
            head: hex(&sample[..sample.len().min(16)]),
            entropy: entropy(&sample),
        });
        c.r.seek(SeekFrom::Start(offset + 8 + len as u64))?;
        if report.chunks.len() > 100_000 {
            report.note = Some("too many chunks; stopped".into());
            break;
        }
    }
    Ok(report)
}

/// The downloaded replay for `match_id`, if there is one: a `.vrf` whose
/// file name or Unreal replay name contains the match id.
pub fn find_for_match(match_id: &str) -> Option<PathBuf> {
    let dir = demos_dir()?;
    let id = match_id.to_ascii_lowercase();
    let files = list(&dir);
    if let Some(p) = files.iter().find(|p| {
        p.file_name()
            .is_some_and(|n| n.to_string_lossy().to_ascii_lowercase().contains(&id))
    }) {
        return Some(p.clone());
    }
    files.into_iter().find(|p| {
        std::fs::File::open(p)
            .and_then(|mut f| inspect_header(&mut f))
            .ok()
            .flatten()
            .is_some_and(|h| h.friendly_name.to_ascii_lowercase().contains(&id))
    })
}

/// Just the Unreal header (reads the first 64 KiB only).
fn inspect_header<R: Read + Seek>(r: &mut R) -> std::io::Result<Option<UeHeader>> {
    let size = r.seek(SeekFrom::End(0))?;
    r.seek(SeekFrom::Start(0))?;
    let mut first = vec![0u8; size.min(64 * 1024) as usize];
    r.read_exact(&mut first)?;
    let mut c = std::io::Cursor::new(first);
    // Same walk as `inspect`, on the in-memory prefix (chunk walk stops early).
    Ok(inspect(&mut c)?.header)
}

/// Human-readable structure report for one replay file.
pub fn describe(path: &Path) -> String {
    use std::fmt::Write;
    let mut out = String::new();
    let _ = writeln!(
        out,
        "file {}",
        path.file_name().map(|n| n.to_string_lossy()).unwrap_or_default()
    );
    let report = match std::fs::File::open(path).and_then(|mut f| inspect(&mut f)) {
        Ok(r) => r,
        Err(e) => {
            let _ = writeln!(out, "  couldn't read it: {e}");
            return out;
        }
    };
    let _ = writeln!(out, "  size {} bytes", report.size);
    let _ = writeln!(out, "  first 64 bytes: {}", report.prefix);
    match &report.header {
        Some(h) => {
            let _ = writeln!(
                out,
                "  unreal header at {}: file version {}, length {} s, network version {}, changelist {}, \
                 compressed {:?}, encrypted {:?}, live {}",
                h.offset,
                h.file_version,
                h.length_ms / 1000,
                h.network_version,
                h.changelist,
                h.compressed,
                h.encrypted,
                h.is_live
            );
        }
        None => {
            let _ = writeln!(out, "  no Unreal replay header found");
        }
    }
    let mut counts = std::collections::BTreeMap::<u32, (usize, u64)>::new();
    for c in &report.chunks {
        let e = counts.entry(c.kind).or_default();
        e.0 += 1;
        e.1 += c.size as u64;
    }
    for (kind, (n, bytes)) in &counts {
        let _ = writeln!(out, "  chunks {:<10} x{n:<5} {bytes} bytes", chunk_name(*kind));
    }
    for kind in counts.keys() {
        if let Some(c) = report.chunks.iter().find(|c| c.kind == *kind) {
            let _ = writeln!(
                out,
                "  first {:<10} @{} size {} entropy {:.2} head {}",
                chunk_name(c.kind),
                c.offset,
                c.size,
                c.entropy,
                c.head
            );
        }
    }
    if let Some(n) = &report.note {
        let _ = writeln!(out, "  note: {n}");
    }
    out
}

/// Report on `path`, or on the two newest files in the Demos folder.
pub fn check_all(path: Option<PathBuf>) -> String {
    let files = match path {
        Some(p) => vec![p],
        None => match demos_dir() {
            Some(dir) => {
                let files = list(&dir);
                if files.is_empty() {
                    return "No downloaded replays yet. In VALORANT, open a match in your history and download its replay.\n"
                        .into();
                }
                files.into_iter().take(2).collect()
            }
            None => {
                return "No replay folder found (%LOCALAPPDATA%\\VALORANT\\Saved\\Demos). \
                        Download a replay from your match history in VALORANT first.\n"
                    .into();
            }
        },
    };
    let mut out = format!("Tracked v{} replay file check\n", env!("CARGO_PKG_VERSION"));
    for p in files {
        out.push_str(&describe(&p));
    }
    out.push_str("(no player names or positions; at most the match id)\n");
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A tiny synthetic file in the Unreal local-replay layout with a
    /// made-up 8-byte vendor marker in front.
    fn sample() -> Vec<u8> {
        let mut v = b"RIOTVRF\0".to_vec();
        let mut put = |x: u32| v.extend_from_slice(&x.to_le_bytes());
        put(UE_REPLAY_MAGIC);
        put(6); // file version
        put(90_000); // length ms
        put(2); // network version
        put(12345); // changelist
        put(5); // FString "demo\0"
        v.extend_from_slice(b"demo\0");
        let mut put = |x: u32| v.extend_from_slice(&x.to_le_bytes());
        put(0); // is live
        v.extend_from_slice(&0i64.to_le_bytes()); // timestamp
        let mut put = |x: u32| v.extend_from_slice(&x.to_le_bytes());
        put(1); // compressed
        put(0); // encrypted
        put(0); // key length
        for (kind, payload) in [(0u32, vec![1u8; 10]), (1, vec![7u8; 300]), (3, vec![2u8; 5])] {
            v.extend_from_slice(&kind.to_le_bytes());
            v.extend_from_slice(&(payload.len() as u32).to_le_bytes());
            v.extend_from_slice(&payload);
        }
        v
    }

    #[test]
    fn walks_the_unreal_layout_after_a_vendor_marker() {
        let r = inspect(&mut std::io::Cursor::new(sample())).unwrap();
        let h = r.header.unwrap();
        assert_eq!(h.offset, 8);
        assert_eq!((h.file_version, h.length_ms, h.changelist), (6, 90_000, 12345));
        assert_eq!(h.friendly_name, "demo");
        assert_eq!((h.compressed, h.encrypted), (Some(true), Some(false)));
        let kinds: Vec<(u32, u32)> = r.chunks.iter().map(|c| (c.kind, c.size)).collect();
        assert_eq!(kinds, vec![(0, 10), (1, 300), (3, 5)]);
        assert!(r.note.is_none());
        assert!(r.chunks[1].entropy < 0.01);
    }

    #[test]
    fn unknown_files_are_reported_not_misread() {
        let r = inspect(&mut std::io::Cursor::new(vec![0xABu8; 500])).unwrap();
        assert!(r.header.is_none());
        assert!(r.note.is_some());
    }
}
