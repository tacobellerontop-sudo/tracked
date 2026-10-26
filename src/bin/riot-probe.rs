//! `riot-probe`: a console check of the Riot connection layer against your
//! real client, milestone by milestone (M1 identity, M2 an authenticated PD
//! call, M3 live game state, M4 survive the client closing and reopening).
//!
//! It prints no passwords or tokens, so the output is safe to share.
//!
//!     riot-probe            # run until Ctrl+C
//!     riot-probe --once     # stop after M2
//!     riot-probe --debug    # also print debug logs to stderr
//!     riot-probe --replay [matchId]
//!                           # 2D replay check on your latest (or given) match:
//!                           # prints round 1's kills and writes replay-check.png

use std::sync::Arc;
use std::time::Duration;

use futures_util::StreamExt;
use tracked::diagnostics::{Step, StepState};
use tracked::riot::{self, lockfile, pd, RiotError, WatchEvent};

fn stamp() -> String {
    chrono::Local::now().format("%H:%M:%S").to_string()
}

fn print_step(step: Step, state: &StepState) {
    let (mark, text) = match state {
        StepState::Pending | StepState::Running => return,
        StepState::Ok(m) => ("ok  ", m.clone()),
        StepState::Warn(m) => ("warn", m.clone()),
        StepState::Failed { message, hint } => ("FAIL", format!("{message}\n                 hint: {hint}")),
    };
    println!("{} [{mark}] {:<30} {text}", stamp(), step.title());
}

#[tokio::main]
async fn main() {
    let args: Vec<String> = std::env::args().collect();
    let once = args.iter().any(|a| a == "--once");
    if args.iter().any(|a| a == "--debug") {
        tracing_subscriber::fmt()
            .with_env_filter(tracked::logging::filter(true))
            .with_writer(std::io::stderr)
            .init();
    }
    println!(
        "Tracked riot-probe v{} — prints no passwords or tokens",
        env!("CARGO_PKG_VERSION")
    );
    let http = riot::public_http();
    let mut last_wait = String::new();

    loop {
        let quiet = !last_wait.is_empty();
        let result = riot::connect_observed(&http, &mut |step, state| {
            // While waiting for the client, don't repeat the same lines.
            if !(quiet && matches!(state, StepState::Failed { .. })) {
                print_step(step, &state);
            }
        })
        .await;
        let session = match result {
            Ok(s) => Arc::new(s),
            Err(e) => {
                let msg = match e {
                    RiotError::ClientNotRunning => "waiting for the Riot Client to start…".to_string(),
                    RiotError::SignedOut => "waiting for you to sign in…".to_string(),
                    other => format!("not connected yet: {other} (retrying)"),
                };
                if msg != last_wait {
                    println!("{} {msg}", stamp());
                    last_wait = msg;
                }
                tokio::time::sleep(Duration::from_millis(2500)).await;
                continue;
            }
        };
        last_wait.clear();

        println!(
            "{} M1 ✓ connected as {} — region {} / shard {}, local TLS: {:?}",
            stamp(),
            session.riot_id(),
            session.region,
            session.shard,
            session.local.backend()
        );
        println!("{}      PD  {}", stamp(), session.hosts.pd);
        println!("{}      GLZ {}", stamp(), session.hosts.glz);

        match pd::player_mmr(&session, &session.puuid).await {
            Ok(m) => {
                let latest = m.latest_competitive_update.unwrap_or_default();
                println!(
                    "{} M2 ✓ 200 from {} — latest competitive tier {} ({} RR)",
                    stamp(),
                    session.hosts.pd,
                    latest.tier_after_update,
                    latest.ranked_rating_after_update
                );
            }
            Err(tracked::error::ApiError::NotFound) => {
                println!("{} M2 ✓ PD answered (no ranked data on this account)", stamp())
            }
            Err(e) => println!("{} M2 ✗ PD call failed: {e}", stamp()),
        }
        if let Some(i) = args.iter().position(|a| a == "--replay") {
            let id = args.get(i + 1).filter(|a| !a.starts_with("--")).cloned();
            replay_check(&session, &http, id).await;
            return;
        }
        if once {
            return;
        }

        println!(
            "{} M3   watching game state; enter agent select or a match (Ctrl+C to quit)",
            stamp()
        );
        let (pid, port) = (session.local.pid(), session.local.port());
        let mut events = std::pin::pin!(riot::watch(session.clone()));
        let mut check = tokio::time::interval(Duration::from_secs(3));
        loop {
            tokio::select! {
                ev = events.next() => match ev {
                    Some(WatchEvent::SocketUp) => println!("{} M3   event socket connected", stamp()),
                    Some(WatchEvent::SocketDown(why)) => println!("{} M3   event socket down ({why}); polling", stamp()),
                    Some(WatchEvent::State { state, presence }) => {
                        let extra = presence
                            .map(|p| format!(" queue={} party={} map={}", p.queue_id, p.party_size, p.map))
                            .unwrap_or_default();
                        println!("{} M3 → {state:?}{extra}", stamp());
                    }
                    // Friends' presences and chat aren't part of the probe's output.
                    Some(WatchEvent::Presences(_) | WatchEvent::Chat(_)) => {}
                    None => break,
                },
                _ = check.tick() => {
                    let alive = matches!(lockfile::read().await, Ok(l) if l.pid == pid && l.port == port);
                    if !alive {
                        println!("{} M4   client closed → Lost; waiting for it to come back", stamp());
                        break;
                    }
                }
            }
        }
        // M4: tokens can be refreshed on demand too.
        let _ = riot::refresh_tokens(&session).await;
    }
}

/// Replay milestones against real data: parse a match (M1), then plot it on
/// the real minimap with valorant-api's calibration (M2) into a PNG.
async fn replay_check(session: &riot::RiotSession, http: &reqwest::Client, id: Option<String>) {
    use tracked::replay::{self, calibrate, Availability};

    let id = match id {
        Some(id) => id,
        None => match pd::match_history(session, &session.puuid, 0, 1).await {
            Ok(h) if !h.history.is_empty() => h.history[0].match_id.clone(),
            Ok(_) => return println!("{} replay ✗ no matches in your history", stamp()),
            Err(e) => return println!("{} replay ✗ match history failed: {e}", stamp()),
        },
    };
    let raw = match replay::fetch::fetch(session, &id).await {
        Ok(r) => r,
        Err(e) => return println!("{} replay ✗ match-details failed: {e}", stamp()),
    };
    let m = replay::from_raw(&raw);
    let kills: usize = m.rounds.iter().map(|r| r.feed.len()).sum();
    let frames: usize = m.rounds.iter().map(|r| r.frames.len()).sum();
    println!(
        "{} replay M1 ✓ {} · {} · queue {} · {} rounds · {kills} kills · {frames} positional frames · {:?}",
        stamp(),
        id,
        m.map_id,
        m.queue_id,
        m.rounds.len(),
        m.availability
    );
    let name = |p: &str| {
        m.player(p)
            .map(|p| p.name.clone())
            .unwrap_or_else(|| p[..8.min(p.len())].to_string())
    };
    if let Some(r) = m.rounds.first() {
        println!("{}      round 1 ({} won, {}):", stamp(), r.winning_team, r.result);
        for e in &r.feed {
            let k = &e.kill;
            println!(
                "{}        {:>2}:{:02}  {} → {}  [{} {}]{}  {}",
                stamp(),
                e.t_ms / 60_000,
                e.t_ms / 1000 % 60,
                name(&k.killer),
                name(&k.victim),
                k.damage.damage_type,
                k.damage.damage_item,
                if k.headshot { " HS" } else { "" },
                match e.frame {
                    Some(i) => format!("{} positions", r.frames[i].positions.len()),
                    None => "no positions".into(),
                }
            );
        }
    }
    if m.availability != Availability::Available {
        return;
    }

    // M2: calibration + minimap from valorant-api.com.
    let maps: serde_json::Value = match http.get("https://valorant-api.com/v1/maps").send().await {
        Ok(r) => r.json().await.unwrap_or_default(),
        Err(e) => return println!("{} replay M2 ✗ valorant-api.com unreachable: {e}", stamp()),
    };
    let Some(map) = maps["data"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|v| v["mapUrl"].as_str() == Some(m.map_id.as_str()))
    else {
        return println!("{} replay M2 ✗ map {} not in valorant-api", stamp(), m.map_id);
    };
    let Some(cal) = tracked::content::valorant_api::map_cal(map) else {
        return println!("{} replay M2 ✗ no calibration for this map", stamp());
    };
    println!("{} replay M2   calibration {cal:?}", stamp());
    let url = map["displayIcon"].as_str().unwrap_or_default();
    let bytes = match http.get(url).send().await {
        Ok(r) => r.bytes().await.unwrap_or_default(),
        Err(e) => return println!("{} replay M2 ✗ minimap download failed: {e}", stamp()),
    };
    let Ok(img) = image::load_from_memory(&bytes) else {
        return println!("{} replay M2 ✗ couldn't decode the minimap", stamp());
    };
    let mut img = img.to_rgba8();
    let (w, h) = (img.width() as f32, img.height() as f32);
    let to_px = |l: &replay::Loc| {
        let (nx, ny) = calibrate::world_to_norm(l, &cal);
        (nx * w, ny * h)
    };
    let mut outside = 0;
    // White: valorant-api's named callouts (should sit inside their areas).
    for c in map["callouts"].as_array().into_iter().flatten() {
        let l = replay::Loc {
            x: c["location"]["x"].as_f64().unwrap_or(0.0) as f32,
            y: c["location"]["y"].as_f64().unwrap_or(0.0) as f32,
        };
        let (x, y) = to_px(&l);
        outside += usize::from(!(0.0..w).contains(&x) || !(0.0..h).contains(&y));
        dot(&mut img, x, y, 6.0, [20, 20, 20, 255]);
        dot(&mut img, x, y, 4.0, [255, 255, 255, 255]);
    }
    // Yellow: every plant in the match (should cluster on the sites).
    for r in &m.rounds {
        for f in r.frames.iter().filter_map(|f| f.marker.as_ref()) {
            let (x, y) = to_px(&f.pos);
            dot(&mut img, x, y, 9.0, [20, 20, 20, 255]);
            dot(&mut img, x, y, 7.0, [255, 205, 0, 255]);
        }
    }
    // Players at round 1's first positional frame, with facing.
    if let Some(f) = m.rounds.first().and_then(|r| r.frames.first()) {
        for p in &f.positions {
            let (x, y) = to_px(&p.pos);
            let color = if p.team == "Red" {
                [235, 70, 85, 255]
            } else {
                [40, 190, 170, 255]
            };
            if let Some(theta) = p.facing {
                let (dx, dy) = calibrate::facing(theta, &cal);
                line(&mut img, (x, y), (x + dx * 28.0, y + dy * 28.0), color);
            }
            dot(&mut img, x, y, 9.0, [20, 20, 20, 255]);
            dot(&mut img, x, y, 7.0, if p.alive { color } else { [140, 140, 140, 255] });
        }
    }
    let path = std::path::Path::new("replay-check.png");
    match img.save(path) {
        Ok(()) => println!(
            "{} replay M2 ✓ wrote {} — white = named callouts, yellow = plants, coloured = players at round 1's first kill \
             ({outside} callouts fell outside the image)",
            stamp(),
            path.display()
        ),
        Err(e) => println!("{} replay M2 ✗ couldn't write the PNG: {e}", stamp()),
    }
}

fn dot(img: &mut image::RgbaImage, cx: f32, cy: f32, r: f32, c: [u8; 4]) {
    let (x0, x1) = ((cx - r).floor() as i64, (cx + r).ceil() as i64);
    let (y0, y1) = ((cy - r).floor() as i64, (cy + r).ceil() as i64);
    for y in y0..=y1 {
        for x in x0..=x1 {
            let (dx, dy) = (x as f32 - cx, y as f32 - cy);
            if dx * dx + dy * dy <= r * r && x >= 0 && y >= 0 && (x as u32) < img.width() && (y as u32) < img.height() {
                img.put_pixel(x as u32, y as u32, image::Rgba(c));
            }
        }
    }
}

fn line(img: &mut image::RgbaImage, a: (f32, f32), b: (f32, f32), c: [u8; 4]) {
    let steps = ((b.0 - a.0).abs().max((b.1 - a.1).abs()) as usize).max(1);
    for i in 0..=steps {
        let t = i as f32 / steps as f32;
        dot(img, a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t, 1.5, c);
    }
}
