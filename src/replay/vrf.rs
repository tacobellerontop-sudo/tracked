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
    files.sort_by(|a, b| b.0.cmp(&a.0));
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
