//! Continuous playback of a round.
//!
//! The match data only pins players down at kills, plants and defuses. To
//! play a round like a video, each player's recorded positions become a
//! track, and in between we draw a smooth curve through them. Those in-between
//! positions are an estimate for viewing, not tracked data. At every recorded
//! moment the pose is exactly what the API reported.

use super::{FrameKind, Loc, RoundReplay};

/// One recorded position of a player.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Sample {
    pub t: f32,
    pub pos: Loc,
    pub facing: Option<f32>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Track {
    pub puuid: String,
    pub team: String,
    pub agent: String,
    /// Alive samples in time order.
    pub samples: Vec<Sample>,
    /// When and where they died, if they did.
    pub death: Option<(f32, Loc)>,
}

/// A player's state at some instant of the round.
#[derive(Debug, Clone, PartialEq)]
pub struct Pose<'a> {
    pub puuid: &'a str,
    pub team: &'a str,
    pub agent: &'a str,
    pub pos: Loc,
    pub facing: Option<f32>,
    pub alive: bool,
    /// Within a few ms of a recorded moment (not interpolated).
    pub exact: bool,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct RoundTimeline {
    /// Where playback starts and ends (ms since round start).
    pub start: f32,
    pub end: f32,
    pub tracks: Vec<Track>,
    /// (time, kind, frame index) for every positional event.
    pub events: Vec<(f32, FrameKind, usize)>,
}

/// Seconds of lead-in before the first event and of tail after the last.
const LEAD_IN: f32 = 3000.0;
const TAIL: f32 = 2500.0;

impl RoundTimeline {
    pub fn build(round: &RoundReplay) -> RoundTimeline {
        let mut tracks: Vec<Track> = Vec::new();
        let mut events = Vec::new();
        for (i, f) in round.frames.iter().enumerate() {
            let t = f.t_ms as f32;
            events.push((t, f.kind, i));
            for p in &f.positions {
                let idx = match tracks.iter().position(|tr| tr.puuid == p.puuid) {
                    Some(idx) => idx,
                    None => {
                        tracks.push(Track {
                            puuid: p.puuid.clone(),
                            team: p.team.clone(),
                            agent: p.agent.clone(),
                            samples: Vec::new(),
                            death: None,
                        });
                        tracks.len() - 1
                    }
                };
                let tr = &mut tracks[idx];
                if p.alive {
                    if tr.death.is_none() {
                        tr.samples.push(Sample {
                            t,
                            pos: p.pos,
                            facing: p.facing,
                        });
                    }
                } else if tr.death.is_none() {
                    tr.death = Some((t, p.pos));
                }
            }
        }
        // Kills that carried no positions still kill the player at that time
        // (where they were last seen).
        for e in &round.feed {
            if e.frame.is_some() {
                continue;
            }
            if let Some(tr) = tracks.iter_mut().find(|tr| tr.puuid == e.kill.victim) {
                let t = e.t_ms as f32;
                if tr.death.is_none_or(|(dt, _)| t < dt) {
                    let at = tr.samples.iter().rev().find(|s| s.t <= t).or(tr.samples.first());
                    if let Some(s) = at {
                        tr.death = Some((t, s.pos));
                    }
                    tr.samples.retain(|s| s.t <= t);
                }
            }
        }
        let first = events.first().map(|e| e.0).unwrap_or(0.0);
        let last = events.last().map(|e| e.0).unwrap_or(0.0);
        RoundTimeline {
            start: (first - LEAD_IN).max(0.0),
            end: last + TAIL,
            tracks,
            events,
        }
    }

    pub fn duration(&self) -> f32 {
        (self.end - self.start).max(1.0)
    }

    /// Every known player's pose at round time `t` (ms).
    pub fn poses_at(&self, t: f32) -> Vec<Pose<'_>> {
        self.tracks.iter().filter_map(|tr| pose(tr, t)).collect()
    }
}

fn pose(tr: &Track, t: f32) -> Option<Pose<'_>> {
    let base = |pos: Loc, facing: Option<f32>, alive: bool, exact: bool| Pose {
        puuid: &tr.puuid,
        team: &tr.team,
        agent: &tr.agent,
        pos,
        facing,
        alive,
        exact,
    };
    if let Some((dt, at)) = tr.death {
        if t >= dt {
            return Some(base(at, None, false, (t - dt).abs() < 5.0));
        }
    }
    // Recorded points, ending at the death spot (they walked there).
    let mut pts: Vec<Sample> = tr.samples.clone();
    if let Some((dt, at)) = tr.death {
        if pts.last().is_none_or(|s| s.t < dt) {
            pts.push(Sample {
                t: dt,
                pos: at,
                facing: pts.last().and_then(|s| s.facing),
            });
        }
    }
    let first = pts.first()?;
    if t <= first.t {
        return Some(base(first.pos, first.facing, true, (first.t - t).abs() < 5.0));
    }
    let last = pts.last()?;
    if t >= last.t {
        return Some(base(last.pos, last.facing, true, (t - last.t).abs() < 5.0));
    }
    let i = pts.windows(2).position(|w| w[0].t <= t && t <= w[1].t)?;
    let (a, b) = (pts[i], pts[i + 1]);
    let span = (b.t - a.t).max(1.0);
    let u = (t - a.t) / span;
    // Velocity at each end from its neighbours (Catmull-Rom style, with
    // real time spacing) so the path bends smoothly through every sample.
    let vel = |k: usize| -> (f32, f32) {
        let prev = pts[k.saturating_sub(1)];
        let next = pts[(k + 1).min(pts.len() - 1)];
        let dt = (next.t - prev.t).max(1.0);
        ((next.pos.x - prev.pos.x) / dt, (next.pos.y - prev.pos.y) / dt)
    };
    let (va, vb) = (vel(i), vel(i + 1));
    let h = |p0: f32, p1: f32, m0: f32, m1: f32| {
        let (u2, u3) = (u * u, u * u * u);
        (2.0 * u3 - 3.0 * u2 + 1.0) * p0
            + (u3 - 2.0 * u2 + u) * m0 * span
            + (-2.0 * u3 + 3.0 * u2) * p1
            + (u3 - u2) * m1 * span
    };
    let pos = Loc {
        x: h(a.pos.x, b.pos.x, va.0, vb.0),
        y: h(a.pos.y, b.pos.y, va.1, vb.1),
    };
    let facing = match (a.facing, b.facing) {
        (Some(fa), Some(fb)) => Some(lerp_angle(fa, fb, u)),
        (f, None) | (None, f) => f,
    };
    let exact = (t - a.t).abs() < 5.0 || (b.t - t).abs() < 5.0;
    Some(base(pos, facing, true, exact))
}

/// Interpolate angles the short way round.
pub fn lerp_angle(a: f32, b: f32, u: f32) -> f32 {
    let tau = std::f32::consts::TAU;
    let mut d = (b - a) % tau;
    if d > tau / 2.0 {
        d -= tau;
    } else if d < -tau / 2.0 {
        d += tau;
    }
    a + d * u
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn angles_take_the_short_way() {
        let pi = std::f32::consts::PI;
        let mid = lerp_angle(pi - 0.1, -pi + 0.1, 0.5);
        assert!((mid.abs() - pi).abs() < 1e-4, "{mid}");
    }
}
