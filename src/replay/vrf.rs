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
/// The magic VALORANT's `.vrf` files start with instead (seen in real files;
/// the rest of the header follows Unreal's layout).
pub const RIOT_REPLAY_MAGIC: u32 = 0x43F4_EFDD;
/// Magic at the start of Unreal's network demo header (the Header chunk).
pub const DEMO_HEADER_MAGIC: u32 = 0x2CF5_A13D;

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
    pub magic: u32,
    pub file_version: u32,
    /// Unreal custom-version entries (file version 7+).
    pub custom_versions: Vec<u32>,
    pub length_ms: u32,
    pub network_version: u32,
    pub changelist: u32,
    pub friendly_name: String,
    pub is_live: bool,
    pub compressed: Option<bool>,
    pub encrypted: Option<bool>,
    pub key_len: u32,
}

#[derive(Debug, Default, Clone, PartialEq)]
pub struct ChunkInfo {
    pub offset: u64,
    pub kind: u32,
    pub size: u32,
    /// First bytes of the payload, hex.
    pub head: String,
    /// Shannon entropy of the first 4 KiB of the data, bits per byte (≈8 =
    /// compressed or encrypted, lower = raw structured data).
    pub entropy: f32,
    /// Checkpoint / Event: the group name (e.g. "checkpoint", event kinds).
    pub group: Option<String>,
    /// ReplayData / Checkpoint / Event: start and end time (ms).
    pub times: Option<(u32, u32)>,
    /// Size of the payload after the chunk's own sub-header.
    pub data_size: Option<u32>,
    /// File offset where that payload starts.
    pub data_offset: u64,
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

/// Find Riot's or Unreal's replay magic within the first 64 KiB.
fn find_magic(buf: &[u8]) -> Option<(usize, u32)> {
    [RIOT_REPLAY_MAGIC, UE_REPLAY_MAGIC]
        .iter()
        .filter_map(|m| {
            let bytes = m.to_le_bytes();
            buf.windows(4).position(|w| w == bytes).map(|at| (at, *m))
        })
        .min_by_key(|(at, _)| *at)
}

/// Inspect a replay: the prefix, the header, and every chunk (type, size,
/// sub-header fields, a short hex head of the data and an entropy estimate).
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
    let Some((at, magic)) = find_magic(&first) else {
        report.note = Some("no replay magic in the first 64 KiB".into());
        return Ok(report);
    };
    r.seek(SeekFrom::Start(at as u64 + 4))?;
    let mut c = Cursor { r };
    let mut h = UeHeader {
        offset: at as u64,
        magic,
        ..Default::default()
    };
    // Unreal's FLocalFileReplayInfo, in serialization order.
    let parsed: std::io::Result<()> = (|| {
        h.file_version = c.u32()?;
        if h.file_version >= 7 {
            let n = c.u32()?;
            if n > 64 {
                return Err(std::io::Error::other(format!("implausible custom version count {n}")));
            }
            for _ in 0..n {
                c.r.seek(SeekFrom::Current(16))?; // GUID
                h.custom_versions.push(c.u32()?);
            }
        }
        h.length_ms = c.u32()?;
        h.network_version = c.u32()?;
        h.changelist = c.u32()?;
        h.friendly_name = c.fstring()?;
        h.is_live = c.u32()? != 0;
        if h.file_version >= 3 {
            let _timestamp = c.i64()?;
        }
        if h.file_version >= 2 {
            h.compressed = Some(c.u32()? != 0);
        }
        if h.file_version >= 6 {
            h.encrypted = Some(c.u32()? != 0);
            h.key_len = c.u32()?;
            if h.key_len > 1024 {
                return Err(std::io::Error::other(format!("implausible key length {}", h.key_len)));
            }
            c.r.seek(SeekFrom::Current(h.key_len as i64))?;
        }
        Ok(())
    })();
    let version = h.file_version;
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
        let end = offset + 8 + len as u64;
        if kind > 16 || end > size {
            report.note = Some(format!(
                "stopped at offset {offset}: chunk type {kind} / size {len} doesn't fit the Unreal layout"
            ));
            break;
        }
        let mut info = ChunkInfo {
            offset,
            kind,
            size: len,
            ..Default::default()
        };
        // Chunk sub-headers (best effort; a failure just leaves them empty).
        let sub: std::io::Result<()> = (|| {
            match kind {
                1 if version >= 4 => {
                    let (t1, t2) = (c.u32()?, c.u32()?);
                    info.times = Some((t1, t2));
                    info.data_size = Some(c.u32()?);
                    // VALORANT adds the in-memory size before the data.
                    let _memory_size = c.u32()?;
                }
                2 | 3 => {
                    let _id = c.fstring()?;
                    info.group = Some(c.fstring()?);
                    let _metadata = c.fstring()?;
                    let (t1, t2) = (c.u32()?, c.u32()?);
                    info.times = Some((t1, t2));
                    info.data_size = Some(c.u32()?);
                }
                _ => {}
            }
            Ok(())
        })();
        if sub.is_err() || c.r.stream_position()? > end {
            info.group = None;
            info.times = None;
            info.data_size = None;
            c.r.seek(SeekFrom::Start(offset + 8))?;
        }
        let here = c.r.stream_position()?;
        info.data_offset = here;
        let mut sample = vec![0u8; ((end - here) as usize).min(4096)];
        c.r.read_exact(&mut sample)?;
        info.head = hex(&sample[..sample.len().min(16)]);
        info.entropy = entropy(&sample);
        report.chunks.push(info);
        c.r.seek(SeekFrom::Start(end))?;
        if report.chunks.len() > 100_000 {
            report.note = Some("too many chunks; stopped".into());
            break;
        }
    }
    Ok(report)
}

/// Unpack one compressed block: `[u32 uncompressed][u32 compressed][Oodle]`.
pub fn unpack(block: &[u8]) -> Result<Vec<u8>, String> {
    if block.len() < 8 {
        return Err("block too short".into());
    }
    let raw = u32::from_le_bytes(block[0..4].try_into().unwrap()) as usize;
    let packed = u32::from_le_bytes(block[4..8].try_into().unwrap()) as usize;
    if packed != block.len() - 8 {
        return Err(format!(
            "compressed size {packed} doesn't match the block ({})",
            block.len() - 8
        ));
    }
    if raw > 256 * 1024 * 1024 {
        return Err(format!("implausible uncompressed size {raw}"));
    }
    if raw == packed {
        // Stored uncompressed.
        return Ok(block[8..].to_vec());
    }
    let mut out = vec![0u8; raw];
    let n = oozextract::Extractor::new()
        .read_from_slice(&block[8..], &mut out)
        .map_err(|e| format!("oodle: {e:?}"))?;
    if n != raw {
        return Err(format!("unpacked {n} bytes, expected {raw}"));
    }
    Ok(out)
}

/// Unpack the first ReplayData and Checkpoint blocks of a file and report
/// whether that worked, plus which Unreal names appear in the data (to see
/// whether movement can be decoded). Prints no player data.
pub fn decode_check(path: &Path, report: &Report) -> String {
    use std::fmt::Write;
    let mut out = String::new();
    for kind in [1u32, 2] {
        let Some(c) = report.chunks.iter().find(|c| c.kind == kind && c.data_size.is_some()) else {
            continue;
        };
        let len = c.data_size.unwrap_or(0) as usize;
        let block = std::fs::File::open(path).and_then(|mut f| {
            f.seek(SeekFrom::Start(c.data_offset))?;
            let mut b = vec![0u8; len];
            f.read_exact(&mut b)?;
            Ok(b)
        });
        let result = block.map_err(|e| e.to_string()).and_then(|b| unpack(&b));
        match result {
            Ok(data) => {
                let marks = [
                    "ReplicatedMovement",
                    "RepMovement",
                    "Location",
                    "Rotation",
                    "Character",
                    "PlayerState",
                    "ShooterGame",
                    "/Game/",
                    "Default__",
                ];
                let found: Vec<String> = marks
                    .iter()
                    .map(|m| (m, count(&data, m.as_bytes())))
                    .filter(|(_, n)| *n > 0)
                    .map(|(m, n)| format!("{m}x{n}"))
                    .collect();
                let _ = writeln!(
                    out,
                    "  unpacked first {}: {} -> {} bytes, entropy {:.2}, names: {}",
                    chunk_name(kind),
                    len,
                    data.len(),
                    entropy(&data[..data.len().min(65536)]),
                    if found.is_empty() {
                        "none".into()
                    } else {
                        found.join(" ")
                    }
                );
            }
            Err(e) => {
                let _ = writeln!(out, "  couldn't unpack first {}: {e}", chunk_name(kind));
            }
        }
    }
    out
}

fn count(hay: &[u8], needle: &[u8]) -> usize {
    hay.windows(needle.len()).filter(|w| *w == needle).count()
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
            let magic = match h.magic {
                RIOT_REPLAY_MAGIC => "riot",
                UE_REPLAY_MAGIC => "unreal",
                _ => "?",
            };
            let _ = writeln!(
                out,
                "  header ({magic} magic at {}): file version {}, custom versions {:?}, length {} s, \
                 network version {}, changelist {}, live {}, compressed {:?}, encrypted {:?} (key {} bytes)",
                h.offset,
                h.file_version,
                h.custom_versions,
                h.length_ms / 1000,
                h.network_version,
                h.changelist,
                h.is_live,
                h.compressed,
                h.encrypted,
                h.key_len
            );
        }
        None => {
            let _ = writeln!(out, "  no replay header found");
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
    // Header chunk: Unreal's network demo header?
    if let Some(c) = report.chunks.iter().find(|c| c.kind == 0) {
        let demo = c.head.starts_with(&hex(&DEMO_HEADER_MAGIC.to_le_bytes()));
        let _ = writeln!(
            out,
            "  Header     size {} entropy {:.2} demo-header magic {} head {}",
            c.size,
            c.entropy,
            if demo { "yes" } else { "no" },
            c.head
        );
    }
    // Replay data: time coverage, and the first chunks' data heads.
    let data: Vec<&ChunkInfo> = report.chunks.iter().filter(|c| c.kind == 1).collect();
    if !data.is_empty() {
        let first = data.iter().filter_map(|c| c.times).map(|t| t.0).min();
        let last = data.iter().filter_map(|c| c.times).map(|t| t.1).max();
        let mean = data.iter().map(|c| c.entropy).sum::<f32>() / data.len() as f32;
        let _ = writeln!(
            out,
            "  ReplayData covers {:?}..{:?} ms, mean entropy {mean:.2}",
            first, last
        );
        for c in data.iter().take(3) {
            let _ = writeln!(
                out,
                "  ReplayData @{} times {:?} data {:?} entropy {:.2} head {}",
                c.offset, c.times, c.data_size, c.entropy, c.head
            );
        }
    }
    // Checkpoints and events: group names and counts (no ids or metadata).
    for kind in [2u32, 3] {
        let mut groups = std::collections::BTreeMap::<String, usize>::new();
        for c in report.chunks.iter().filter(|c| c.kind == kind) {
            *groups.entry(c.group.clone().unwrap_or_else(|| "?".into())).or_default() += 1;
        }
        if !groups.is_empty() {
            let _ = writeln!(out, "  {} groups {:?}", chunk_name(kind), groups);
        }
        if let Some(c) = report.chunks.iter().find(|c| c.kind == kind) {
            let _ = writeln!(
                out,
                "  first {} times {:?} data {:?} entropy {:.2} head {}",
                chunk_name(kind),
                c.times,
                c.data_size,
                c.entropy,
                c.head
            );
        }
    }
    if report.header.as_ref().is_some_and(|h| h.compressed == Some(true)) {
        out.push_str(&decode_check(path, &report));
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
    fn reads_the_riot_layout_seen_in_real_files() {
        // Header bytes as reported from a real VALORANT .vrf (version 7,
        // one custom version, 29 min, fixed-size UTF-16 name), followed by
        // the remaining fields and one ReplayData chunk.
        let mut v = Vec::new();
        let put = |v: &mut Vec<u8>, x: u32| v.extend_from_slice(&x.to_le_bytes());
        put(&mut v, RIOT_REPLAY_MAGIC);
        put(&mut v, 7);
        put(&mut v, 1);
        v.extend_from_slice(&[0x3e; 16]);
        put(&mut v, 7);
        put(&mut v, 1_759_195);
        put(&mut v, 0x1CA7_EFE6);
        put(&mut v, 5_760_300);
        put(&mut v, (-257i32) as u32);
        let mut name: Vec<u16> = "99436d8f-0820-41f4-ad79-c6874183701b".encode_utf16().collect();
        name.resize(257, 0);
        for u in name {
            v.extend_from_slice(&u.to_le_bytes());
        }
        put(&mut v, 0); // live
        v.extend_from_slice(&0i64.to_le_bytes());
        put(&mut v, 1); // compressed
        put(&mut v, 0); // encrypted
        put(&mut v, 0); // key
        put(&mut v, 1); // ReplayData: t1, t2, size, memory size, data
        put(&mut v, 16 + 28);
        put(&mut v, 0);
        put(&mut v, 5000);
        put(&mut v, 28);
        put(&mut v, 20);
        put(&mut v, 20); // block: uncompressed 20, compressed 20 (stored)
        put(&mut v, 20);
        v.extend_from_slice(&[9u8; 20]);
        let r = inspect(&mut std::io::Cursor::new(v)).unwrap();
        let h = r.header.unwrap();
        assert_eq!(
            (h.magic, h.file_version, h.custom_versions.clone()),
            (RIOT_REPLAY_MAGIC, 7, vec![7])
        );
        assert_eq!(h.length_ms / 60_000, 29);
        assert_eq!(h.changelist, 5_760_300);
        assert_eq!(h.friendly_name, "99436d8f-0820-41f4-ad79-c6874183701b");
        assert_eq!(h.compressed, Some(true));
        assert_eq!(r.chunks.len(), 1);
        assert_eq!(r.chunks[0].times, Some((0, 5000)));
        assert_eq!(r.chunks[0].data_size, Some(28));
        assert!(r.note.is_none(), "{:?}", r.note);
    }

    #[test]
    fn unpack_checks_block_sizes() {
        let mut stored = Vec::new();
        stored.extend_from_slice(&5u32.to_le_bytes());
        stored.extend_from_slice(&5u32.to_le_bytes());
        stored.extend_from_slice(b"hello");
        assert_eq!(unpack(&stored).unwrap(), b"hello");
        // Size fields that don't match the block are refused, not trusted.
        let mut bad = stored.clone();
        bad[4] = 9;
        assert!(unpack(&bad).is_err());
        assert!(unpack(&[1, 2, 3]).is_err());
        // Garbage that claims to be Oodle fails cleanly instead of panicking.
        let mut junk = Vec::new();
        junk.extend_from_slice(&100u32.to_le_bytes());
        junk.extend_from_slice(&6u32.to_le_bytes());
        junk.extend_from_slice(&[0x8c, 0x0a, 1, 2, 3, 4]);
        assert!(unpack(&junk).is_err());
    }

    #[test]
    fn unknown_files_are_reported_not_misread() {
        let r = inspect(&mut std::io::Cursor::new(vec![0xABu8; 500])).unwrap();
        assert!(r.header.is_none());
        assert!(r.note.is_some());
    }
}
