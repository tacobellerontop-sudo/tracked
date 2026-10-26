# Architecture

## The Intent / Update model

Everything is Rust in one process. There's no web layer and no IPC bridge. The
app is split into two halves joined by two channels (`src/ipc.rs`):

```
 UI thread (egui)                               worker thread (tokio)
 ───────────────                                ─────────────────────
 eframe::App::update() every frame              supervisor loop (worker/mod.rs)
   1. drain Update channel → AppState             • dispatches Intents → tokio tasks
   2. draw AppState (ui/*)                        • lockfile → session state machine
   3. user action → Intent  ───── Intent ─────▶   • presence websocket / polling
                            ◀──── Update ─────    • GLZ live polling
      (request_repaint() wakes the UI)            routines in worker/tasks.rs
```

- **`Intent`** (UI → worker, `tokio::mpsc::UnboundedSender`): one variant per user
  action or background need: `Bootstrap`, `RefreshLiveMatch`, `RefreshCareer`,
  `LoadMatchDetails`, `LookupPlayer`, `LookupPuuid`, `ProbePartyMmr`, `SaveSettings`,
  `CheckForUpdates`, `Shutdown`.
- **`Update`** (worker → UI, `std::sync::mpsc::Sender` wrapped in `UpdateSink`, which
  calls `egui::Context::request_repaint()` after each push): `ClientStatus`, `Content`,
  `Game`, `LiveMatch`, `Names`, `Probe`, `Career`, `MatchDetails`, `PlayerLookup`,
  `UpdateStatus`, `Error`…
- **`AppState`** (`app.rs`) is plain data owned by the UI thread. The UI never blocks
  on I/O and never shares a lock with the worker. Loading states are flags/`Option`s
  that updates fill in.
- Inside the worker, shared caches live in `worker::tasks::Ctx` (an `Arc` holding
  the session, catalog, name cache, match cache and probe cache).

The original Tauri command list maps onto this as follows:

| Tauri command | Here |
| --- | --- |
| `riot_status`, `local_riot_id` | supervisor → `Update::ClientStatus` |
| `get_match_state`, `get_pregame`, `get_live_roster`, `get_party` | `tasks::refresh_live` → `Update::LiveMatch` |
| `probe_player_mmr` | `Intent::ProbePartyMmr` → `Update::Probe` / `Update::Names` |
| `tracker_self_rank`, `tracker_competitive_updates`, `tracker_match_history` | `Intent::RefreshCareer` → `Update::Career` |
| `tracker_match_details`, `tracker_match_summary` | `Intent::LoadMatchDetails` → `Update::MatchDetails` |
| `tracker_profile` | `Intent::LookupPlayer` / `LookupPuuid` → `Update::PlayerLookup` |
| `settings_get` / `settings_set` | `config::load` at startup / `Intent::SaveSettings` |

## Riot auth flow

```
lockfile  (%LOCALAPPDATA%\Riot Games\Riot Client\Config\lockfile)
  name:pid:port:password:protocol
      │
      ▼
local API  https://127.0.0.1:<port>   basic auth riot:<password>
  TLS: self-signed → LoopbackOnlyVerifier accepts it ONLY for loopback IPs
  GET /entitlements/v1/token              → accessToken, token (entitlements JWT), subject (PUUID)
  GET /product-session/v1/external-sessions → "-ares-deployment=<region>"  (fallback: /riotclient/region-locale)
  GET /chat/v1/session                    → game_name, game_tag
  wss://127.0.0.1:<port>  [5,"OnJsonApiEvent_chat_v4_presences"] → base64 private blob → MENUS/PREGAME/INGAME
      │
      ▼
Riot web APIs, each request carries:
  Authorization: Bearer <accessToken>
  X-Riot-Entitlements-JWT: <token>
  X-Riot-ClientPlatform: base64({"platformType":"PC",…})
  X-Riot-ClientVersion: <valorant-api.com /v1/version → riotClientVersion; fallback ShooterGame.log>

  PD   https://pd.<shard>.a.pvp.net            MMR, competitive updates, match history/details, names
  GLZ  https://glz-<region>-1.<shard>.a.pvp.net  parties, pregame, core-game  (host regex-validated)
```

- Region → shard: `latam`/`br` → `na`; others map to themselves.
- `Session::send` refuses to attach auth headers to any host that isn't a validated
  PD/GLZ host, refreshes tokens once on 400 `BAD_CLAIMS`/401, honours `Retry-After` on
  429 (capped), and limits concurrency to 4.
- Credentials live in `Secret<T>` (`Debug`/`Display` print `***`, no `Serialize`).
  Token-bearing header values are marked sensitive, and transport crates are capped at
  `warn` in the log filter, because `tungstenite` logs raw handshake headers at TRACE.

## Connection state machine (`worker/mod.rs`)

```
Disconnected ──lockfile found──▶ Session::connect ──ok──▶ Authed{pid,port}
     ▲   │ (poll every 2.5 s)          │ err: backoff 2 s → 30 s       │ every 3 s: lockfile still same?
     │   └── no lockfile: NotRunning ◀─┘                                │
     └───────────────────────── client gone ───────────────────────────┘
Authed:
  presence websocket (reconnect with backoff; poll /chat/v4/presences every 5 s while it's down)
  GLZ live poll: every `poll_interval` (default 5 s) in agent select / in game, every 30 s in menus,
                 immediately on any presence state change
  INGAME → MENUS: refresh career (match just ended)
```

## Module map

| Path | Role |
| --- | --- |
| `main.rs` | flags (`--debug`, `--demo`), logging, eframe bootstrap, spawns worker |
| `app.rs` | `eframe::App`, `AppState`, applying `Update`s |
| `ipc.rs` | `Intent`, `Update`, channel wiring, UI-facing data types |
| `worker/mod.rs` | tokio runtime, supervisor loop, connection + polling state machine |
| `worker/tasks.rs` | async routines behind each intent; shared `Ctx` caches |
| `worker/demo.rs` | `--demo`: synthetic data through the same `Update` path |
| `riot/lockfile.rs` | find + parse the lockfile |
| `riot/tls.rs` | loopback-only certificate verifier |
| `riot/local.rs` | local REST client |
| `riot/auth.rs` | tokens + the four required headers |
| `riot/region.rs` | region/shard, PD/GLZ host builders + validation |
| `riot/mod.rs` | `Session`: authenticated, rate-limited request path |
| `riot/pd.rs` | MMR, competitive updates, history, details, rank snapshot, rank probe |
| `riot/glz.rs` | party / pregame / core-game → `LiveMatch` |
| `riot/presence.rs` | presence blob decode + websocket task |
| `riot/names.rs` | batched PUUID → Riot ID with cache |
| `riot/match_stats.rs` | raw match → `MatchDetail` with derived stats (KAST, trades, clutches…) |
| `riot/account.rs` | optional official account-v1 lookup (`RIOT_API_KEY`) |
| `riot/models.rs` | serde structs (raw Riot payloads + normalized models) |
| `content/valorant_api.rs` | agents, maps, tiers, current act, client version; on-disk icon cache |
| `crosshair.rs` | crosshair code parse / edit / serialize |
| `config.rs` | persisted non-secret settings |
| `logging.rs` | tracing setup with hard caps on transport crates |
| `ui/*` | one module per screen + shared widgets |

## Adding a feature

1. Add an `Intent` variant (if the user triggers it) and an `Update` variant (for
   what the UI displays).
2. Write the async routine in `worker/tasks.rs`, sending results through `ctx.sink`.
3. Dispatch it in `Supervisor::handle` (and in `worker/demo.rs` if it should demo).
4. Apply the `Update` in `AppState::apply`, then draw it in a `ui/` module.
