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

## The Riot connection layer (`riot/`)

Public API (`riot/mod.rs`):

```rust
riot::connect() -> Result<RiotSession, RiotError>          // §2-§6
riot::connect_observed(http, observe)                       // same, reporting each step
riot::watch_state(Arc<RiotSession>) -> impl Stream<Item = GameState>   // §7, de-duplicated
riot::watch(Arc<RiotSession>) -> impl Stream<Item = WatchEvent>        // + socket status, presence details
riot::refresh_tokens(&RiotSession) -> Result<(), RiotError>            // §8
```

`refresh_tokens` takes `&RiotSession`, not `&mut`: the session is shared between
tasks behind an `Arc`, so tokens and the client version sit behind a lock inside it.

```
lockfile  (%LOCALAPPDATA%\Riot Games\Riot Client\Config\lockfile)    lockfile.rs
  name:pid:port:password:protocol    missing → ClientNotRunning, bad → LockfileMalformed
      │
      ▼
local API  https://127.0.0.1:<port>   Authorization: Basic base64("riot:<pw>")    local.rs (LocalAuth)
  TLS: rustls + LoopbackOnlyVerifier (accepts any cert, only for loopback IPs);
       on Windows, falls back once to schannel with cert checks off if rustls's handshake fails
  retries network errors briefly (the client refuses connections right after launch)
  GET /chat/v1/session          → puuid, game_name, game_tag, region, loaded   (404/!loaded → SignedOut)
  GET /entitlements/v1/token    → accessToken, token, subject (must equal the chat puuid)
  region: GET /product-session/v1/external-sessions (-ares-deployment / -config-endpoint)
          → GET /riotclient/region-locale → chat region → region claim in the RSO token
      │
      ▼
Region/Shard enums → Hosts (regex-validated)                                       region.rs
  PD  https://pd.<shard>.a.pvp.net        GLZ https://glz-<region>-1.<shard>.a.pvp.net
  latam/br → shard na; na may be shard pbe
      │
      ▼
PublicAuth::apply(): Authorization: Bearer, X-Riot-Entitlements-JWT,               auth.rs
  X-Riot-ClientPlatform (base64 platform JSON), X-Riot-ClientVersion
  version: TRACKED_CLIENT_VERSION → valorant-api.com → ShooterGame.log; refreshed hourly  version.rs
      │
      ▼
RiotSession::send (session.rs): auth headers only to validated PD/GLZ hosts;
  401/403 or 400 BAD_CLAIMS → refresh_tokens + retry once; version-looking 400 → refetch
  version + retry once; 429 → Retry-After (capped); at most 4 requests in flight
```

Live state (`state.rs`): `wss://127.0.0.1:<port>` with the same basic credentials (sent as
a header, not as `riot:<pw>@` in the URL), subscribe `[5,"OnJsonApiEvent_chat_v4_presences"]`,
decode our entry's base64 `private` blob → `sessionLoopState` → `GameState`. On entering
PREGAME/INGAME the match id comes from GLZ (`/pregame|core-game/v1/players/{puuid}`). If
the socket drops, it polls `GET /chat/v4/presences` every 3 s and retries the socket with
exponential backoff (1 s → 30 s). Socket errors never end the stream.

Lifecycle (`worker/mod.rs`, brief §9): `NotRunning` ("VALORANT isn't running") →
`SignedOut` ("Waiting for you to sign in…") → `Connecting` ("Connecting…") → `Authed`
("Connected as name#tag") → `Lost` (lockfile gone/changed or the port stops answering
twice) → `NotRunning`. Not running and signed out are polled every 2.5 s; other failures
back off 2 s → 30 s.

`riot-probe` (`src/bin/riot-probe.rs`) runs milestones M1–M4 against a real client from a
console and prints no secrets. `tests/riot_layer.rs` runs the same milestones against a fake
client.

## Optional backend (Phase 3)

`backend/` talks to an Tracked-compatible backend at a configurable base URL, using its
own bearer JWT (memory only). It has no reference to the Riot `Session`, so Riot
credentials can't reach it. `worker/social.rs` owns the lifecycle:

```
BackendConfigure ─▶ validate URL ─▶ GET min-version (gate) ─▶ GET app-config ─▶ SignedOut
BackendSignIn    ─▶ POST /api/auth/dev-login ─▶ SignedIn ─▶ friends + presence + lobbies + DM list
                 ─▶ social websocket task (reconnect w/ backoff) + REST heartbeat every 60 s
game state change ─▶ presence_usage   (if share_presence)
party change      ─▶ lobby_push       (if share_lobby; size + queue only)
```

| REST | Purpose |
| --- | --- |
| `GET /api/companion/app-config`, `GET /api/companion/min-version` | remote flags, version gate |
| `POST /api/companion/heartbeat`, `POST /api/companion/feedback` | keep-alive, feedback |
| `POST /api/companion/lobby` (+ `/touch`, `/lookup`, `/clear`) | publish / read lobbies |
| `POST /api/companion/presence/lookup` | batch presence |
| `GET /social/v1/friends` | friends |
| `GET/POST /social/v1/dm`, `GET/POST /social/v1/dm/:id/messages`, `/rename`, `/read`, `/typing`, `/report`, `/requests/:id` | DMs |
| `/mm/v1/...`, `/mm/v2/...` | stats service (canned in the mock) |

Social websocket (`/social/ws`, JSON frames with a `type` field):

| Client → server | Server → client |
| --- | --- |
| `subscribe {topics}` | `hello {user_id}` |
| `heartbeat` (every 25 s) | `heartbeat_ack`, `subscribed` |
| `presence_lookup {user_ids}` | `presence {entries}` |
| `presence_usage {state, queue, party_size}` | `lobby {user_id, lobby}` |
| `lobby_push {lobby}` / `lobby_touch` | `dm {conversation_id, message}`, `typing {...}`, `conversation {...}`, `error` |

`mock-backend/` is an independent axum implementation of the same contract, written
against the JSON wire format rather than the client's Rust types, so the two check each
other. `tests/social_e2e.rs` runs the real worker against it in-process.

## Module map

| Path | Role |
| --- | --- |
| `main.rs` | flags (`--debug`, `--demo`), logging, eframe bootstrap, spawns worker |
| `app.rs` | `eframe::App`, `AppState`, applying `Update`s |
| `ipc.rs` | `Intent`, `Update`, channel wiring, UI-facing data types |
| `worker/mod.rs` | tokio runtime, supervisor loop, connection + polling state machine |
| `worker/tasks.rs` | async routines behind each intent; shared `Ctx` caches |
| `worker/demo.rs` | `--demo`: synthetic data through the same `Update` path |
| `riot/session.rs` | `RiotSession`, `connect`, `refresh_tokens`, the authenticated request path |
| `riot/state.rs` | `GameState`, `watch` / `watch_state` (socket + polling fallback) |
| `riot/version.rs` | client version sources and refresh rules |
| `riot/error.rs` | `RiotError` |
| `riot/lockfile.rs` | find + parse the lockfile |
| `riot/tls.rs` | loopback-only certificate verifier |
| `riot/local.rs` | local REST client |
| `riot/auth.rs` | tokens + the four required headers |
| `riot/region.rs` | region/shard, PD/GLZ host builders + validation |
| `riot/mod.rs` | public API re-exports, `RiotId`, the public HTTP client |
| `riot/pd.rs` | MMR, competitive updates, history, details, rank snapshot, rank probe |
| `riot/glz.rs` | party / pregame / core-game → `LiveMatch` |
| `riot/presence.rs` | presence blob decode |
| `replay/mod.rs` | 2D replay model and `from_raw` / `build_round` (frames per round, dead set, feed) |
| `replay/fetch.rs` | match-details serde models (valapidocs field names) + fetch |
| `replay/timeline.rs` | continuous playback: per-player tracks, curve between recorded positions, poses at any time |
| `replay/calibrate.rs` | `MapCal`, `world_to_norm` (note the x/y swap), facing vectors |
| `riot/chat.rs` | local chat API: friends, requests, conversations, history, send; friend status decode |
| `riot/names.rs` | batched PUUID → Riot ID with cache |
| `riot/match_stats.rs` | raw match → `MatchDetail` with derived stats (KAST, trades, clutches…) |
| `riot/account.rs` | optional official account-v1 lookup (`RIOT_API_KEY`) |
| `riot/models.rs` | serde structs (raw Riot payloads + normalized models) |
| `content/valorant_api.rs` | agents, maps, tiers, current act, client version; on-disk icon cache |
| `crosshair.rs` | crosshair code parse / edit / serialize |
| `config.rs` | persisted non-secret settings |
| `logging.rs` | tracing setup with hard caps on transport crates |
| `backend/client.rs` | backend REST client (base-URL validation, bearer auth) |
| `backend/social_ws.rs` | social websocket protocol + session task |
| `backend/models.rs` | backend wire types, `MinVersionState` |
| `worker/social.rs` | backend lifecycle, socket supervision, DM/feedback intents |
| `worker/friends.rs` | built-in Riot friends/whispers intents (message bodies are never logged) |
| `ui/*` | one module per screen + shared widgets; `ui/theme.rs` holds fonts (Inter, Phosphor icons), palettes and animation helpers |
| `mock-backend/` | axum mock of the backend (`cargo run -p mock-backend`) |

## Adding a feature

1. Add an `Intent` variant (if the user triggers it) and an `Update` variant (for
   what the UI displays).
2. Write the async routine in `worker/tasks.rs`, sending results through `ctx.sink`.
3. Dispatch it in `Supervisor::handle` (and in `worker/demo.rs` if it should demo).
4. Apply the `Update` in `AppState::apply`, then draw it in a `ui/` module.

## Rendering

`main.rs` starts eframe with the wgpu renderer (Direct3D 12 on Windows, Vulkan elsewhere).
winit can't create a second event loop in one process, so if wgpu fails the app relaunches
itself with `--opengl`. Settings → Renderer can force OpenGL. The active adapter is shown
in Settings. Text uses Inter (SIL OFL, `assets/fonts/Inter-LICENSE.txt`); icons use
Phosphor (MIT, via `egui-phosphor`) in their own `icons` font family, because Inter and
Phosphor both define glyphs in the private-use area.
