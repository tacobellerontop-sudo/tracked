//! VALORANT crosshair share codes.
//!
//! A code is a `;`-separated list. It starts with `0` (format version and
//! general settings), followed by optional sections introduced by `P`
//! (primary), `A` (ADS) and `S` (sniper). Inside a section, tokens come in
//! `key;value` pairs, e.g. `0;P;c;5;h;0;0l;4;0o;2;0a;1;0f;0;1b;0`.
//!
//! We keep the parsed pairs in order so unknown keys survive a round trip.

use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Section {
    /// "0" for the general section, otherwise "P", "A" or "S".
    pub name: String,
    pub pairs: Vec<(String, String)>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CrosshairCode {
    pub sections: Vec<Section>,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CrosshairError {
    #[error("the code is empty")]
    Empty,
    #[error("codes start with \"0;\"")]
    BadVersion,
    #[error("\"{0}\" has no value")]
    MissingValue(String),
    #[error("empty value or key")]
    EmptyToken,
    #[error("section {0} appears twice")]
    DuplicateSection(String),
}

const SECTION_MARKERS: &[&str] = &["P", "A", "S"];

impl CrosshairCode {
    pub fn parse(code: &str) -> Result<Self, CrosshairError> {
        let code = code.trim().trim_end_matches(';');
        if code.is_empty() {
            return Err(CrosshairError::Empty);
        }
        let mut tokens = code.split(';').map(str::trim);
        if tokens.next() != Some("0") {
            return Err(CrosshairError::BadVersion);
        }
        let mut sections = vec![Section {
            name: "0".into(),
            pairs: Vec::new(),
        }];
        while let Some(tok) = tokens.next() {
            if tok.is_empty() {
                return Err(CrosshairError::EmptyToken);
            }
            if SECTION_MARKERS.contains(&tok) {
                if sections.iter().any(|s| s.name == tok) {
                    return Err(CrosshairError::DuplicateSection(tok.into()));
                }
                sections.push(Section {
                    name: tok.into(),
                    pairs: Vec::new(),
                });
                continue;
            }
            let value = tokens.next().ok_or_else(|| CrosshairError::MissingValue(tok.into()))?;
            if value.is_empty() {
                return Err(CrosshairError::EmptyToken);
            }
            sections.last_mut().unwrap().pairs.push((tok.into(), value.into()));
        }
        Ok(Self { sections })
    }

    pub fn section(&self, name: &str) -> Option<&Section> {
        self.sections.iter().find(|s| s.name == name)
    }

    pub fn get(&self, section: &str, key: &str) -> Option<&str> {
        self.section(section)?
            .pairs
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_str())
    }

    /// Set a value, creating the section/key as needed.
    pub fn set(&mut self, section: &str, key: &str, value: impl Into<String>) {
        let value = value.into();
        if self.section(section).is_none() {
            self.sections.push(Section {
                name: section.into(),
                pairs: Vec::new(),
            });
        }
        let s = self.sections.iter_mut().find(|s| s.name == section).unwrap();
        match s.pairs.iter_mut().find(|(k, _)| k == key) {
            Some((_, v)) => *v = value,
            None => s.pairs.push((key.into(), value)),
        }
    }

    fn num(&self, key: &str, default: f32) -> f32 {
        self.get("P", key).and_then(|v| v.parse().ok()).unwrap_or(default)
    }

    fn flag(&self, key: &str, default: bool) -> bool {
        self.get("P", key).map(|v| v == "1").unwrap_or(default)
    }

    /// The primary crosshair as typed parameters (with game defaults).
    pub fn primary(&self) -> Primary {
        let color_index = self.num("c", 0.0) as u8;
        let custom = self.get("P", "u").and_then(parse_hex_rgba);
        Primary {
            color: if color_index == 8 {
                custom.unwrap_or([255; 4])
            } else {
                preset_color(color_index)
            },
            color_index,
            outlines: self.flag("h", true),
            outline_thickness: self.num("t", 1.0),
            outline_opacity: self.num("o", 0.5),
            center_dot: self.flag("d", false),
            dot_thickness: self.num("z", 2.0),
            dot_opacity: self.num("a", 1.0),
            inner: Lines {
                show: self.flag("0b", true),
                thickness: self.num("0t", 2.0),
                length: self.num("0l", 6.0),
                offset: self.num("0o", 3.0),
                opacity: self.num("0a", 0.8),
            },
            outer: Lines {
                show: self.flag("1b", true),
                thickness: self.num("1t", 2.0),
                length: self.num("1l", 2.0),
                offset: self.num("1o", 10.0),
                opacity: self.num("1a", 0.35),
            },
        }
    }

    /// Write typed primary parameters back into the code.
    pub fn set_primary(&mut self, p: &Primary) {
        let b = |v: bool| if v { "1" } else { "0" };
        self.set("P", "c", p.color_index.to_string());
        self.set("P", "h", b(p.outlines));
        self.set("P", "t", fmt_num(p.outline_thickness));
        self.set("P", "o", fmt_num(p.outline_opacity));
        self.set("P", "d", b(p.center_dot));
        self.set("P", "z", fmt_num(p.dot_thickness));
        self.set("P", "a", fmt_num(p.dot_opacity));
        for (prefix, l) in [("0", &p.inner), ("1", &p.outer)] {
            self.set("P", &format!("{prefix}b"), b(l.show));
            self.set("P", &format!("{prefix}t"), fmt_num(l.thickness));
            self.set("P", &format!("{prefix}l"), fmt_num(l.length));
            self.set("P", &format!("{prefix}o"), fmt_num(l.offset));
            self.set("P", &format!("{prefix}a"), fmt_num(l.opacity));
        }
    }
}

impl Default for CrosshairCode {
    fn default() -> Self {
        Self {
            sections: vec![Section {
                name: "0".into(),
                pairs: Vec::new(),
            }],
        }
    }
}

impl fmt::Display for CrosshairCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut parts: Vec<&str> = Vec::new();
        for s in &self.sections {
            parts.push(&s.name);
            for (k, v) in &s.pairs {
                parts.push(k);
                parts.push(v);
            }
        }
        f.write_str(&parts.join(";"))
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Lines {
    pub show: bool,
    pub thickness: f32,
    pub length: f32,
    pub offset: f32,
    pub opacity: f32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Primary {
    pub color: [u8; 4],
    pub color_index: u8,
    pub outlines: bool,
    pub outline_thickness: f32,
    pub outline_opacity: f32,
    pub center_dot: bool,
    pub dot_thickness: f32,
    pub dot_opacity: f32,
    pub inner: Lines,
    pub outer: Lines,
}

pub const COLOR_NAMES: &[&str] = &[
    "White",
    "Green",
    "Yellow Green",
    "Green Yellow",
    "Yellow",
    "Cyan",
    "Pink",
    "Red",
    "Custom",
];

pub fn preset_color(i: u8) -> [u8; 4] {
    match i {
        1 => [0, 255, 0, 255],
        2 => [127, 255, 0, 255],
        3 => [223, 255, 0, 255],
        4 => [255, 255, 0, 255],
        5 => [0, 255, 255, 255],
        6 => [255, 0, 255, 255],
        7 => [255, 0, 0, 255],
        _ => [255, 255, 255, 255],
    }
}

fn parse_hex_rgba(s: &str) -> Option<[u8; 4]> {
    let s = s.trim_start_matches('#');
    if s.len() != 8 && s.len() != 6 {
        return None;
    }
    let byte = |i: usize| u8::from_str_radix(s.get(i..i + 2)?, 16).ok();
    Some([byte(0)?, byte(2)?, byte(4)?, if s.len() == 8 { byte(6)? } else { 255 }])
}

/// `2.0` -> "2", `0.35` -> "0.35".
pub fn fmt_num(v: f32) -> String {
    let s = format!("{:.3}", v);
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s.is_empty() || s == "-0" {
        "0".into()
    } else {
        s.into()
    }
}
