# Architecture

## Processes

| Process | Lifetime | Responsibility |
| --- | --- | --- |
| `sky-linux` (Tauri) | while the user wants a window or tray icon | Rendering only. Starts the daemon if it isn't running. |
| `sky-daemon` | whole login session (systemd user unit) | Owns Sky Core: accounts, transfers, backups, history, persistence. |
| `rclone rcd` | child of the daemon, restarted with backoff | All cloud I/O through its JSON remote-control API. |

Closing the window only hides it. Quitting from the tray stops the UI, and backups keep running. Only stopping `sky-daemon.service` stops background work.

## Sky Core modules (`crates/sky-core/src`)

| Module | Purpose |
| --- | --- |
| `rclone::process` | Spawns `rclone rcd` on a unix socket in `$XDG_RUNTIME_DIR/sky-linux` (0700), with random per-launch RC credentials passed by environment. Clears the environment so a user's personal `RCLONE_*` settings can't leak in. Encrypts `rclone.conf` on first start. Restarts rclone with exponential backoff. Captures logs, which is how the OAuth URL is surfaced. |
| `rclone::client` | Typed HTTP-over-unix-socket client for the RC methods used: `operations/list/about/mkdir/movefile/deletefile/copyfile`, `sync/copy`, `sync/move`, `job/*`, `core/stats`, `config/*`. |
| `rclone::config_flow` | Drives rclone's non-interactive config state machine (`config/create` → `config/update --continue`). It answers known questions (`config_type=traditional`, `config_traditional=telia_no`, `config_is_local=true`) and accepts defaults for anything new, so small backend changes don't break onboarding. |
| `oauth` | Browser sign-in (authorization code + PKCE) run by Sky Core so the page after sign-in is Sky Linux's own, translated, with no mention of rclone. Uses rclone's parameters per white-label (realm, client id, scopes, redirect `http://localhost:53682/`); endpoints come from OpenID discovery. The token is handed to rclone, which refreshes it from then on. |
| `accounts` | Connect (browser sign-in, then a verification `about` call), adopt an existing remote, disconnect. Disconnecting never deletes files. |
| `transfers` | Each upload or download is an rclone async job with its own stats group. A 1 s poll loop updates progress and persists state. The transfer spec is stored as JSON so interrupted transfers resume exactly. Downloads pick non-conflicting names. |
| `backup` | Upload-only jobs that validate paths (system folders are refused). They run on inotify changes with a debounce (falling back to polling if watches are exhausted), on an interval, or manually. Runs of the same job are serialised. A missing source folder, such as an unplugged drive, is skipped and never treated as "everything was deleted". |
| `sync` | Two-way sync on `rclone bisync`. The first run is a non-destructive merge (`resync`, newer wins). Local files removed or overwritten by sync are moved to a per-pair recovery folder (`backupDir1`). A `.sky-linux-sync` marker on both sides (`checkAccess`) stops sync if the folder vanished or was replaced. Runs that delete more than 50 % stop with *needs review* (restore everything / allow deletions). Conflicts keep both versions (`name.conflict1.ext`). Runs on inotify changes (3 s debounce) plus polling every 5 min for cloud changes; runs of one pair are serialised; lost bisync state is rebuilt with another merge. |
| `db` | SQLite (WAL) with forward-only migrations: settings, accounts, backup jobs, transfers, history, the metadata cache, and a reserved `sync_pairs` table. |
| `events` | Typed event bus. Every state change is broadcast to UIs, and user-visible events are stored as history. |
| `error` | `CoreError` → `UserError` translation (expired sign-in, offline, storage full, permissions, rate limiting), keeping the technical details. |
| `secrets` | `SecretStore` trait backed by the Secret Service (`keyring` crate), with an in-memory fallback. |
| `ipc` | Daemon protocol: newline-delimited JSON over a 0600 unix socket, with `SO_PEERCRED` UID checks. A typed `Request` enum, concurrent request handling, pushed events, and a client used by the Tauri shell. |
| `service` | `SkyCore` façade that wires everything together and adds the metadata cache (30 s TTL, stale data served when offline) and auth-expiry detection. |

## IPC protocol

```jsonc
→ {"id": 7, "method": "files.list", "params": {"path": "Photos"}}
← {"id": 7, "result": [{"name": "2025", "path": "Photos/2025", "kind": "folder", ...}]}
← {"id": 8, "error": {"code": "auth_expired", "title": "Please sign in again", "message": "…", "action": "sign_in_again", "technical": "…"}}
← {"event": {"type": "transferUpdated", "data": {...}}}
```

Methods: `daemon.status`, `accounts.{list,providers,connect,disconnect}`, `files.{list,search,usage,mkdir,move,delete}`, `transfers.{upload,download,list,cancel}`, `backups.{list,create,setEnabled,delete,runNow}`, `sync.{list,create,setEnabled,delete,runNow,resolve}`, `history.list`, `settings.{get,set}` (allowlisted keys only).

The TypeScript mirror of the models lives in `apps/desktop/src/lib/types.ts`.

## Desktop app (`apps/desktop`)

- React 19 + TypeScript with Tailwind 4. Components are written in shadcn/ui style (cva, `cn`, tokens in `styles.css`) and support light and dark themes.
- TanStack Query holds daemon data, and daemon events are folded into the cache (`lib/hooks.ts`). Zustand holds UI state: view, navigation history, selection, toasts and live stats.
- `lib/api.ts` is the only gateway. Inside Tauri it calls the `daemon_request` command. In a plain browser it falls back to `lib/demo.ts`, a full in-memory backend for UI work and screenshots.
- The Tauri capability file grants only dialog, notification, autostart and opener. The webview has no shell or filesystem access.

## Provider independence

`Provider` is a parameter everywhere. Adding another Jottacloud white-label (Elkjøp, Telia Cloud SE, Jottacloud itself, …) means testing it and flipping `is_supported`. Supporting a non-Jottacloud backend means adding a new answerer for its rclone config flow.
