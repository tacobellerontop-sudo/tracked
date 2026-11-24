//! What can be read out of a `.vrf` replay's network stream.
//!
//! The replay data is Unreal's demo format: frames of packets, packets of
//! bunches, bunches on channels, one channel per actor. VALORANT encrypts
//! every property and RPC payload (player movement included), but the
//! *spawn info* of each actor is outside those payloads: when an actor
//! channel opens, its class and world location are sent in the clear. That
//! gives exact positions and times for ability objects (smokes, walls,
//! traps, …), death markers (`*_PostDeath_PC_C`), dropped weapons and
//! match-start positions. Round starts come from the file's event chunks.
//!
//! Everything here is a bounds-checked reader over untrusted bytes: a
//! malformed packet is skipped, never trusted.

use std::collections::HashMap;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

use super::vrf;

/// LSB-first bit reader (Unreal's `FBitReader`).
struct Bits<'a> {
    d: &'a [u8],
    pos: usize,
    end: usize,
    err: bool,
}

impl<'a> Bits<'a> {
    fn new(d: &'a [u8], nbits: usize) -> Self {
        Bits {
            d,
            pos: 0,
            end: nbits.min(d.len() * 8),
            err: false,
        }
    }
    fn left(&self) -> usize {
        self.end.saturating_sub(self.pos)
    }
    fn bit(&mut self) -> bool {
        if self.pos >= self.end {
            self.err = true;
            return false;
        }
        let v = self.d[self.pos >> 3] >> (self.pos & 7) & 1;
        self.pos += 1;
        v == 1
    }
    fn bits(&mut self, n: u32) -> u64 {
        let mut v = 0u64;
        for i in 0..n {
            if self.bit() {
                v |= 1 << i;
            }
        }
        v
    }
    /// `SerializeInt(value, max)`.
    fn int_max(&mut self, max: u32) -> u32 {
        let (mut v, mut mask) = (0u32, 1u32);
        while v + mask < max && mask != 0 {
            if self.bit() {
                v |= mask;
            }
            mask <<= 1;
        }
        v
    }
    /// `SerializeIntPacked`: 7 bits per byte, low bit = more follows.
    fn packed(&mut self) -> u32 {
        let mut v = 0u32;
        for i in 0..5 {
            let b = self.bits(8) as u32;
            v |= (b >> 1) << (7 * i);
            if b & 1 == 0 {
                break;
            }
        }
        v
    }
    fn skip(&mut self, n: usize) {
        self.pos += n;
        if self.pos > self.end {
            self.err = true;
        }
    }
    fn skip_fstring(&mut self) {
        let len = self.bits(32) as u32 as i32;
        if !(-4096..=4096).contains(&len) {
            self.err = true;
            return;
        }
        let bytes = if len < 0 { -len as usize * 2 } else { len as usize };
        self.skip(bytes * 8);
    }
    fn f32(&mut self) -> f64 {
        f32::from_bits(self.bits(32) as u32) as f64
    }
    fn f64(&mut self) -> f64 {
        f64::from_bits(self.bits(64))
    }
}

/// Byte-aligned reader for the frame layer.
struct Bytes<'a> {
    d: &'a [u8],
    pos: usize,
}

impl<'a> Bytes<'a> {
    fn take(&mut self, n: usize) -> Option<&'a [u8]> {
        let s = self.d.get(self.pos..self.pos.checked_add(n)?)?;
        self.pos += n;
        Some(s)
    }
    fn u8(&mut self) -> Option<u8> {
        self.take(1).map(|b| b[0])
    }
    fn u32(&mut self) -> Option<u32> {
        self.take(4).map(|b| u32::from_le_bytes(b.try_into().unwrap()))
    }
    fn f32(&mut self) -> Option<f32> {
        self.u32().map(f32::from_bits)
    }
    fn packed(&mut self) -> Option<u32> {
        let mut v = 0u32;
        for i in 0..5 {
            let b = self.u8()? as u32;
            v |= (b >> 1) << (7 * i);
            if b & 1 == 0 {
                return Some(v);
            }
        }
        Some(v)
    }
    fn fstring(&mut self) -> Option<String> {
        let len = self.u32()? as i32;
        if !(-4096..=4096).contains(&len) {
            return None;
        }
        Some(if len >= 0 {
            String::from_utf8_lossy(self.take(len as usize)?)
                .trim_end_matches('\0')
                .to_string()
        } else {
            let b = self.take(-len as usize * 2)?;
            let units: Vec<u16> = b.chunks(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
            String::from_utf16_lossy(&units).trim_end_matches('\0').to_string()
        })
    }
    /// An exported net field name: hardcoded index or string + number.
    fn skip_name(&mut self) -> Option<()> {
        if self.u8()? != 0 {
            self.packed()?;
        } else {
            self.fstring()?;
            self.u32()?;
        }
        Some(())
    }
}

/// NetGUID → (outer, path), from the frames' GUID exports.
#[derive(Default)]
struct Guids(HashMap<u32, (u32, String)>);

impl Guids {
    fn load(&mut self, r: &mut Bytes, depth: u32) -> Option<u32> {
        let g = r.packed()?;
        if g == 0 || depth > 16 {
            return Some(g);
        }
        let flags = r.u8()?;
        if flags & 1 != 0 {
            let outer = self.load(r, depth + 1)?;
            let path = r.fstring()?;
            if flags & 4 != 0 {
                r.u32()?;
            }
            self.0.insert(g, (outer, path));
        }
        Some(g)
    }
    fn name(&self, g: u32) -> Option<&str> {
        self.0.get(&g).map(|(_, p)| p.as_str())
    }
    /// The outermost path (the package, e.g. `/Game/Characters/Smonk/...`).
    fn package(&self, mut g: u32) -> Option<&str> {
        let mut path = None;
        for _ in 0..16 {
            let (outer, p) = self.0.get(&g)?;
            path = Some(p.as_str());
            if *outer == 0 {
                break;
            }
            g = *outer;
        }
        path
    }
}

/// One actor whose channel opened with a location.
#[derive(Debug, Clone, PartialEq)]
pub struct Spawn {
    /// Replay time, ms since the recording started.
    pub t_ms: u32,
    /// Archetype object name, e.g. `Default__GameObject_Smonk_NewSmoke_C`.
    pub archetype: String,
    /// Its package path, e.g. `/Game/Characters/Smonk/S0/Ability_E/...`.
    pub package: String,
    pub pos: [f64; 3],
    /// When its channel closed because the actor was destroyed (a smoke
    /// fading, a weapon picked up). `None` if it lasted to the end or went
    /// dormant.
    pub end_ms: Option<u32>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Stream {
    pub spawns: Vec<Spawn>,
    /// Replay time of each round's start (from the file's event chunks).
    pub round_starts: Vec<u32>,
    /// Replay time of every character death (event chunks).
    pub deaths: Vec<u32>,
    pub packets: usize,
    pub bad_packets: usize,
}

/// Parser state carried across chunks.
#[derive(Default)]
struct State {
    guids: Guids,
    /// Open channel → index of its spawn in `Stream::spawns`.
    channels: HashMap<u32, usize>,
}

/// Unreal's `EChannelCloseReason::Destroyed`.
const CLOSE_DESTROYED: u32 = 0;

/// Read one decompressed ReplayData chunk's frames.
fn read_frames(data: &[u8], st: &mut State, out: &mut Stream) -> Option<()> {
    let mut r = Bytes { d: data, pos: 0 };
    while r.pos < data.len() {
        r.u32()?; // level index
        let t_ms = (r.f32()?.max(0.0) * 1000.0) as u32;
        // Net field exports (property names; payloads are encrypted, so
        // they're only skipped).
        for _ in 0..r.packed()? {
            r.packed()?; // path index
            if r.packed()? != 0 {
                r.fstring()?;
                r.packed()?; // export count
            }
            if r.u8()? != 0 {
                r.packed()?; // handle
                r.u32()?; // checksum
                r.skip_name()?;
            }
        }
        for _ in 0..r.packed()? {
            let len = r.u32()? as usize;
            let blob = r.take(len)?;
            st.guids.load(&mut Bytes { d: blob, pos: 0 }, 0);
        }
        for _ in 0..r.packed()? {
            r.fstring()?; // streaming level
        }
        let external = r.u32()? as usize;
        r.u32()?;
        r.take(external)?;
        loop {
            r.packed()?; // seen level index
            let size = r.u32()? as usize;
            if size == 0 {
                break;
            }
            let packet = r.take(size)?;
            out.packets += 1;
            if packet_spawns(packet, t_ms, st, out).is_none() {
                out.bad_packets += 1;
            }
        }
    }
    Some(())
}

/// Walk a packet's bunches; record actor spawns from channel-open bunches.
fn packet_spawns(packet: &[u8], t_ms: u32, st: &mut State, out: &mut Stream) -> Option<()> {
    // The last set bit terminates the packet.
    let last = *packet.last()?;
    if last == 0 {
        return None;
    }
    let nbits = (packet.len() - 1) * 8 + (7 - last.leading_zeros() as usize);
    let mut r = Bits::new(packet, nbits);
    while r.left() > 0 {
        let control = r.bit();
        let (mut open, mut close, mut reason) = (false, false, 0);
        if control {
            open = r.bit();
            close = r.bit();
            if close {
                reason = r.int_max(15);
            }
        }
        r.bit(); // replication paused
        let reliable = r.bit();
        let channel = r.packed();
        let exports = r.bit();
        let must_map = r.bit();
        let partial = r.bit();
        r.bit(); // VALORANT-specific flag (not in stock Unreal)
        let (mut initial, mut last) = (true, true);
        if partial {
            initial = r.bit();
            last = r.bit();
        }
        if reliable || open {
            if r.bit() {
                r.packed();
            } else {
                r.skip_fstring();
                r.skip(32);
            }
        }
        let len = r.int_max(1024 * 8 * 2) as usize;
        if r.err || len > r.left() {
            return None;
        }
        let start = r.pos;
        if open && initial && !exports {
            let mut b = Bits {
                d: packet,
                pos: start,
                end: start + len,
                err: false,
            };
            if let Some(s) = new_actor(&mut b, must_map, t_ms, &st.guids) {
                st.channels.insert(channel, out.spawns.len());
                out.spawns.push(s);
            }
        }
        if close && last {
            if let Some(i) = st.channels.remove(&channel) {
                if reason == CLOSE_DESTROYED {
                    out.spawns[i].end_ms = Some(t_ms);
                }
            }
        }
        r.pos = start + len;
    }
    (!r.err).then_some(())
}

/// Unreal's `SerializeNewActor`: NetGUID, archetype, level, then optional
/// location (quantized or full doubles), rotation, scale and velocity.
fn new_actor(r: &mut Bits, must_map: bool, t_ms: u32, guids: &Guids) -> Option<Spawn> {
    if must_map {
        let n = r.bits(16);
        for _ in 0..n {
            r.packed();
        }
    }
    let load = |r: &mut Bits| {
        let g = r.packed();
        if g == 1 {
            r.bits(8); // the default GUID carries export flags
        }
        g
    };
    let actor = load(r);
    if actor == 0 || actor % 2 == 1 {
        return None; // static actor: no spawn info
    }
    let archetype = load(r);
    load(r); // level
    if !r.bit() {
        return None; // no location
    }
    let pos = if r.bit() {
        quantized_vector(r, 10.0)
    } else {
        [r.f64(), r.f64(), r.f64()]
    };
    if r.err || pos.iter().any(|v| !v.is_finite() || v.abs() > 1e7) {
        return None;
    }
    Some(Spawn {
        t_ms,
        archetype: guids.name(archetype)?.to_string(),
        package: guids.package(archetype).unwrap_or_default().to_string(),
        pos,
        end_ms: None,
    })
}

/// UE5 quantized vector (large-world-coordinate format).
fn quantized_vector(r: &mut Bits, scale: f64) -> [f64; 3] {
    let info = r.int_max(1 << 7);
    let bits = info & 63;
    let scaled = info >> 6 != 0;
    if bits > 0 {
        let sign = 1i64 << (bits - 1);
        let mut v = [0.0; 3];
        for c in &mut v {
            let x = r.bits(bits) as i64;
            *c = ((x ^ sign) - sign) as f64;
            if scaled {
                *c /= scale;
            }
        }
        v
    } else if scaled {
        [r.f64(), r.f64(), r.f64()]
    } else {
        [r.f32(), r.f32(), r.f32()]
    }
}

/// Read every spawn and round start from a replay file.
pub fn read(path: &Path) -> std::io::Result<Stream> {
    let mut f = std::fs::File::open(path)?;
    let report = vrf::inspect(&mut f)?;
    let mut out = Stream::default();
    let mut st = State::default();
    for c in &report.chunks {
        let Some(len) = c.data_size else { continue };
        let mut block = vec![0u8; len as usize];
        f.seek(SeekFrom::Start(c.data_offset))?;
        f.read_exact(&mut block)?;
        match c.kind {
            1 => {
                let Ok(data) = vrf::unpack(&block) else { continue };
                read_frames(&data, &mut st, &mut out);
            }
            3 => match (c.group.as_deref(), c.times) {
                (Some("roundStarted"), Some((t, _))) => out.round_starts.push(t),
                (Some("characterDeath"), Some((t, _))) => out.deaths.push(t),
                _ => {}
            },
            _ => {}
        }
    }
    out.round_starts.sort_unstable();
    out.deaths.sort_unstable();
    Ok(out)
}

/// What a spawn is, by its archetype: an ability object placed on the map,
/// a dropped weapon, or something not shown (weapons in hand, armor, orbs,
/// agents' equipped abilities, …).
pub fn classify(s: &Spawn) -> Option<super::ItemKind> {
    let class = s.archetype.strip_prefix("Default__").unwrap_or(&s.archetype);
    let class = class.strip_suffix("_C").unwrap_or(class);
    if class == "EquippableGroundPickup" {
        return Some(super::ItemKind::DroppedWeapon);
    }
    let agent = s.package.strip_prefix("/Game/Characters/")?.split('/').next()?;
    if agent.starts_with('_') {
        return None;
    }
    // Objects an ability leaves in the world. Projectiles are skipped: they
    // spawn where they were thrown from, not where they land.
    let (_, rest) = ["GameObject_", "Gameobject_", "Pawn_", "AIPawn_", "Patch_"]
        .iter()
        .find_map(|p| class.strip_prefix(p).map(|r| (p, r)))?;
    if rest.contains("ForObservers") {
        return None;
    }
    Some(super::ItemKind::Ability {
        agent: agent.to_string(),
        name: pretty_name(rest, agent),
    })
}

/// `Smonk_E_NewSmoke_Production` → "New Smoke".
fn pretty_name(rest: &str, agent: &str) -> String {
    let skip = |w: &str| {
        w.eq_ignore_ascii_case(agent) || matches!(w, "Q" | "E" | "C" | "X" | "4" | "Production" | "V2" | "PDS")
    };
    let mut out = String::new();
    for word in rest.split('_').filter(|w| !w.is_empty() && !skip(w)) {
        for (i, ch) in word.char_indices() {
            let prev = word[..i].chars().last();
            if i > 0 && ch.is_uppercase() && prev.is_some_and(|p| p.is_lowercase()) {
                out.push(' ');
            }
            out.push(ch);
        }
        out.push(' ');
    }
    out.trim().to_string()
}

fn median(mut v: Vec<f32>) -> Option<f32> {
    if v.is_empty() {
        return None;
    }
    v.sort_by(f32::total_cmp);
    Some(v[v.len() / 2])
}

/// Put the replay file's abilities and drops into the match's rounds.
///
/// Rounds are split at the file's round-start events. The file's clock and
/// Riot's round clock differ by the buy phase, so each round's offset is
/// measured from its deaths: the file's death events against the API's kill
/// times, in order (median of the differences). Rounds without kills use the
/// match's typical offset.
pub fn attach(m: &mut super::MatchReplay, s: &Stream) {
    let starts = &s.round_starts;
    if starts.is_empty() {
        return;
    }
    let bounds = |i: usize| (starts[i], starts.get(i + 1).copied().unwrap_or(u32::MAX));
    let mut offsets: Vec<Option<f32>> = Vec::new();
    for (i, round) in m.rounds.iter().enumerate().take(starts.len()) {
        let (a, b) = bounds(i);
        let deaths: Vec<f32> = s
            .deaths
            .iter()
            .filter(|d| (a..b).contains(*d))
            .map(|d| (d - a) as f32)
            .collect();
        let mut kills: Vec<f32> = round.feed.iter().map(|k| k.t_ms as f32).collect();
        kills.sort_by(f32::total_cmp);
        offsets.push(median(deaths.iter().zip(&kills).map(|(d, k)| d - k).collect()));
    }
    let typical = median(offsets.iter().flatten().copied().collect()).unwrap_or(0.0);
    for (i, round) in m.rounds.iter_mut().enumerate().take(starts.len()) {
        let (a, b) = bounds(i);
        let off = offsets[i].unwrap_or(typical);
        let at = |t: u32| (t - a) as f32 - off;
        round.items = s
            .spawns
            .iter()
            .filter(|x| (a..b).contains(&x.t_ms))
            .filter_map(|x| {
                Some(super::MapItem {
                    kind: classify(x)?,
                    pos: super::Loc {
                        x: x.pos[0] as f32,
                        y: x.pos[1] as f32,
                    },
                    t: at(x.t_ms).max(0.0),
                    end: x.end_ms.filter(|e| *e < b).map(|e| at(e).max(0.0)),
                })
            })
            .filter(|it| it.end.is_none_or(|e| e > 0.0))
            .collect();
        // Start playback early enough to see utility set up before the
        // first kill.
        if let Some(first) = round.items.iter().map(|it| it.t).min_by(f32::total_cmp) {
            let tl = &mut round.timeline;
            if !tl.tracks.is_empty() {
                tl.start = tl.start.min((first - 1000.0).max(0.0));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Bits written LSB-first, the way Unreal's `FBitWriter` does.
    #[derive(Default)]
    struct W {
        d: Vec<u8>,
        n: usize,
    }
    impl W {
        fn bit(&mut self, b: bool) {
            if self.n.is_multiple_of(8) {
                self.d.push(0);
            }
            if b {
                self.d[self.n / 8] |= 1 << (self.n % 8);
            }
            self.n += 1;
        }
        fn bits(&mut self, v: u64, n: u32) {
            for i in 0..n {
                self.bit(v >> i & 1 == 1);
            }
        }
        fn int_max(&mut self, v: u32, max: u32) {
            let (mut acc, mut mask) = (0u32, 1u32);
            while acc + mask < max && mask != 0 {
                self.bit(v & mask != 0);
                acc |= v & mask;
                mask <<= 1;
            }
        }
        fn packed(&mut self, mut v: u32) {
            loop {
                let more = v >= 0x80;
                self.bits((((v & 0x7f) << 1) | more as u32) as u64, 8);
                v >>= 7;
                if !more {
                    break;
                }
            }
        }
    }

    fn packed_bytes(out: &mut Vec<u8>, mut v: u32) {
        loop {
            let more = v >= 0x80;
            out.push((((v & 0x7f) << 1) | more as u32) as u8);
            v >>= 7;
            if !more {
                break;
            }
        }
    }

    fn fstring(out: &mut Vec<u8>, s: &str) {
        out.extend_from_slice(&(s.len() as u32 + 1).to_le_bytes());
        out.extend_from_slice(s.as_bytes());
        out.push(0);
    }

    /// One bunch on `ch`: an actor open (with spawn info) or a close.
    fn bunch(w: &mut W, ch: u32, open: Option<(u32, u32, [i64; 3])>, close: bool) {
        w.bit(true); // control
        w.bit(open.is_some());
        w.bit(close);
        if close {
            w.int_max(0, 15); // destroyed
        }
        w.bit(false); // paused
        w.bit(true); // reliable
        w.packed(ch);
        w.bit(false); // exports
        w.bit(false); // must-be-mapped
        w.bit(false); // partial
        w.bit(false); // VALORANT flag
        w.bit(true); // hardcoded channel name
        w.packed(102); // NAME_Actor
        let mut body = W::default();
        if let Some((actor, arch, pos)) = open {
            body.packed(actor);
            body.packed(arch);
            body.packed(0); // level
            body.bit(true); // has location
            body.bit(true); // quantized
            body.int_max(20 | 64, 128); // 20 bits per component, scaled
            for c in pos {
                body.bits((c * 10) as u64 & 0xFFFFF, 20);
            }
            body.bit(false); // rotation
            body.bit(false); // scale
            body.bit(false); // velocity
        }
        w.int_max(body.n as u32, 1024 * 8 * 2);
        for i in 0..body.n {
            w.bit(body.d[i / 8] >> (i % 8) & 1 == 1);
        }
    }

    fn packet(w: W) -> Vec<u8> {
        let mut w = w;
        w.bit(true); // terminator
        w.d
    }

    fn frame(out: &mut Vec<u8>, t: f32, guids: &[(u32, u32, &str)], packets: &[Vec<u8>]) {
        out.extend_from_slice(&0u32.to_le_bytes());
        out.extend_from_slice(&t.to_le_bytes());
        out.push(0); // no field exports
        packed_bytes(out, guids.len() as u32);
        for (g, outer, path) in guids {
            let mut blob = Vec::new();
            packed_bytes(&mut blob, *g);
            blob.push(1); // has path
            packed_bytes(&mut blob, *outer);
            if *outer != 0 {
                blob.push(0); // outer already known: no path
            }
            fstring(&mut blob, path);
            out.extend_from_slice(&(blob.len() as u32).to_le_bytes());
            out.extend_from_slice(&blob);
        }
        out.push(0); // no streaming levels
        out.extend_from_slice(&1u64.to_le_bytes());
        out.push(0); // external data terminator
        for p in packets {
            out.push(0);
            out.extend_from_slice(&(p.len() as u32).to_le_bytes());
            out.extend_from_slice(p);
        }
        out.push(0);
        out.extend_from_slice(&0u32.to_le_bytes());
    }

    #[test]
    fn reads_spawns_and_lifetimes_from_frames() {
        let smoke_pkg = "/Game/Characters/Smonk/S0/Ability_E/GameObject_Smonk_NewSmoke";
        let mut data = Vec::new();
        let mut w = W::default();
        bunch(&mut w, 5, Some((40, 9, [1234, -5678, 300])), false);
        bunch(&mut w, 6, Some((42, 13, [10, 20, 30])), false);
        frame(
            &mut data,
            1.5,
            &[(11, 0, smoke_pkg), (9, 11, "Default__GameObject_Smonk_NewSmoke_C")],
            &[packet(w)],
        );
        let mut w = W::default();
        bunch(&mut w, 5, None, true);
        frame(&mut data, 16.5, &[], &[packet(w)]);

        let mut st = State::default();
        let mut out = Stream::default();
        read_frames(&data, &mut st, &mut out).unwrap();
        assert_eq!((out.packets, out.bad_packets), (2, 0));
        // The second actor's archetype (13) was never exported: skipped.
        assert_eq!(out.spawns.len(), 1);
        let s = &out.spawns[0];
        assert_eq!(s.archetype, "Default__GameObject_Smonk_NewSmoke_C");
        assert_eq!(s.package, smoke_pkg);
        assert_eq!(s.pos, [1234.0, -5678.0, 300.0]);
        assert_eq!((s.t_ms, s.end_ms), (1500, Some(16500)));
        assert_eq!(
            classify(s),
            Some(crate::replay::ItemKind::Ability {
                agent: "Smonk".into(),
                name: "New Smoke".into()
            })
        );
    }

    #[test]
    fn garbage_packets_are_counted_not_trusted() {
        let mut data = Vec::new();
        frame(&mut data, 0.0, &[], &[vec![0xff; 40], vec![0x00]]);
        let mut st = State::default();
        let mut out = Stream::default();
        read_frames(&data, &mut st, &mut out).unwrap();
        assert_eq!(out.packets, 2);
        assert!(out.spawns.is_empty());
        // Truncated frames stop the chunk instead of panicking.
        assert!(read_frames(&data[..data.len() - 3], &mut st, &mut Stream::default()).is_none());
    }

    #[test]
    fn classifies_what_is_shown() {
        let s = |arch: &str, pkg: &str| Spawn {
            t_ms: 0,
            archetype: arch.into(),
            package: pkg.into(),
            pos: [0.0; 3],
            end_ms: None,
        };
        use crate::replay::ItemKind;
        assert_eq!(
            classify(&s(
                "Default__EquippableGroundPickup_C",
                "/Game/Weapons/WeaponPickups/EquippableGroundPickup"
            )),
            Some(ItemKind::DroppedWeapon)
        );
        assert_eq!(
            classify(&s(
                "Default__Pawn_Killjoy_E_Turret_C",
                "/Game/Characters/Killjoy/S0/Ability_E/Pawn_Killjoy_E_Turret"
            )),
            Some(ItemKind::Ability {
                agent: "Killjoy".into(),
                name: "Turret".into()
            })
        );
        for (arch, pkg) in [
            (
                "Default__AssaultRifle_AK_C",
                "/Game/Equippables/Guns/Rifles/AK/AssaultRifle_AK",
            ),
            (
                "Default__Ability_Killjoy_E_Turret_C",
                "/Game/Characters/Killjoy/S0/Ability_E/Ability_Killjoy_E_Turret",
            ),
            (
                "Default__Projectile_Smonk_DecayNade_C",
                "/Game/Characters/Smonk/S0/Ability_Q/Projectile_Smonk_DecayNade",
            ),
            ("Default__Killjoy_PC_C", "/Game/Characters/Killjoy/Killjoy_PC"),
            (
                "Default__UltPointOrb_C",
                "/Game/GameObjects/CollectibleOrbs/UltPointOrb",
            ),
        ] {
            assert_eq!(classify(&s(arch, pkg)), None, "{arch}");
        }
    }
}
