//! `riot-probe`: a console check of the Riot connection layer against your
//! real client, milestone by milestone (M1 identity, M2 an authenticated PD
//! call, M3 live game state, M4 survive the client closing and reopening).
//!
//! It prints no passwords or tokens, so the output is safe to share.
//!
//!     riot-probe            # run until Ctrl+C
//!     riot-probe --once     # stop after M2
//!     riot-probe --debug    # also print debug logs to stderr

use std::sync::Arc;
use std::time::Duration;

use ascend_lite::diagnostics::{Step, StepState};
use ascend_lite::riot::{self, lockfile, pd, RiotError, WatchEvent};
use futures_util::StreamExt;

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
            .with_env_filter(ascend_lite::logging::filter(true))
            .with_writer(std::io::stderr)
            .init();
    }
    println!(
        "Ascend-Lite riot-probe v{} — prints no passwords or tokens",
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
            Err(ascend_lite::error::ApiError::NotFound) => {
                println!("{} M2 ✓ PD answered (no ranked data on this account)", stamp())
            }
            Err(e) => println!("{} M2 ✗ PD call failed: {e}", stamp()),
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
