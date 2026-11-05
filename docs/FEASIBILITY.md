# Phase 0: feasibility

The goal is to prove that rclone can authenticate with a real Telia Sky account, browse, upload and download files, and refresh tokens reliably.

## Verified without an account (rclone v1.75.2)

| Question | Finding |
| --- | --- |
| Does rclone know Telia Sky? | **Yes.** The `jottacloud` backend's "traditional" auth offers the white-label `telia_no` ("Telia Sky (Norway)"). Others include `telia_se`, `elkjop`, `tele2`, `onlime`, `phonero`, `jottacloud`, `gigantti`, `elgiganten_*`, `elko` and `mediamarkt`. |
| Which auth flow? | `config_type=traditional` → `config_traditional=telia_no` → browser OAuth (`config_is_local=true`) against `sky-auth.telia.no` (Keycloak realm `get`, client id `desktop`). The "standard" flow (personal login token) is the official Jottacloud route and is not offered by Telia Sky. |
| Can it run without a terminal? | **Yes.** The whole flow runs through RC `config/create` + `config/update` with `nonInteractive`. The OAuth step blocks while the user signs in in the browser; the URL is logged, and Sky Core shows it in the UI. |
| Can rclone's RC API be locked down? | **Yes.** `rclone rcd --rc-addr unix:///…` works; requests without the per-launch credentials get `401`. |
| Can the token store be encrypted without prompts? | **Yes.** `rclone config encryption set --password-command "printenv VAR"` creates the encrypted config. `RCLONE_CONFIG_PASS` then keeps it encrypted across RC writes and token refreshes. This is covered by `rclone_config_is_encrypted_at_rest`. |
| Upload, download, move and copy semantics | Covered by the end-to-end tests in `crates/sky-core/tests/rclone_integration.rs` against a real rclone process (alias remote). `sync/copy` never deletes at the destination. |
| Is bisync usable over RC? | **Yes, with one trap:** over RC bisync's delete limit defaults to **0 %**, so any deletion aborts with "too many deletes". Sky Core always sends `maxDelete` explicitly. `backupDir1`, `checkAccess`, `conflictResolve`/`conflictLoser` and `resilient`/`recover` all work as documented. |
| Do RC filters work for search? | `_filter.IncludeRule` applies to files only, and directories are always listed, so Sky Core filters names after the listing. |

## Needs a real Telia Sky account (run `scripts/feasibility.sh`)

First run on a real Telia Sky account (2026-11-04, rclone v1.75.2, CachyOS):

- [x] Browser sign-in via Telia's own sign-in page completes with no terminal input (`telia_no`, traditional auth).
- [x] rclone's page after sign-in ("Success! … go back to rclone") can't be customised for Jottacloud: there's no setting, and `rclone authorize --template` answers "not supported by this backend". So Sky Core now runs the browser sign-in itself (`crates/sky-core/src/oauth.rs`), using rclone's exact parameters for `telia_no`, and passes the token to rclone, which then skips its own browser step ("Token already configured – replace it?" → no). Verified against the real server up to Telia's login page: discovery works and Telia accepts the client, redirect URI and PKCE, forwarding to `identity.teliacompany.com` (`CIAM-NO-Telia-Sky`). **Still to confirm with a real account:** the redirect back and the token exchange (`sky-daemon sign-in`).
- [x] `about` works, but returns **only `used`** (12.9 GB) and no `total`, possibly because the plan is unlimited. The UI shows space used without a bar when there's no total.
- [x] **Where the files are:** the account has one device (`Jotta`) with the areas `Archive`, `Photos`, `Shared` and `Sync`. All files (4,142, 35.6 GB) were in `Photos` (the phone backup); the other three were empty. rclone listed and sized `Photos` without problems. rclone has no whole-device mode, so Sky Core joins all areas into one rclone `combine` remote whose upstreams are connection strings on the signed-in remote (`sky-telia-no,mountpoint=Photos:`); every path starts with its area. `Photos` and `Shared` are read-only in Sky Linux for now.
- [ ] **Token sharing across areas:** the four areas share one token. If Telia rotates refresh tokens on every use, two areas refreshing at the same moment could invalidate each other's refresh token. Verify with a real account by letting the token expire and then listing two areas in parallel.
- [x] (history) **The default device/mountpoint (`Jotta` / `Archive`) was empty although the account holds 12.9 GB.** The files live in another device or mountpoint. The script now sets `config_non_standard=true` and lists the account's devices and mountpoints to find them. Sky Core's sign-in still takes the default and must be changed once we know where Telia Sky keeps user files (and which area the Telia apps show as "Mine filer").
- [x] 5 MB and unicode-filename (æøå) round trip, byte-identical, 1.4 s; server-side MD5 matches; purge goes to the trash.
- [ ] Forced token expiry: not tested yet. The script's config rewrite lowercased `configVersion`, so rclone rejected the config as "outdated" (a script bug, now fixed).
- [ ] **Single active authentication:** rclone warns that traditional auth allows only one active session. Telia has no Linux client, but its Windows/macOS desktop client uses the same OAuth client id (`desktop`). Does signing in here sign out that client (for people who also use a Mac or dual-boot), the mobile apps, or other rclone instances?
- [ ] Refresh-token lifetime when idle (re-run after a week)
- [ ] Rate limits and behaviour for large uploads (multi-GB, resumable `upload_resume_limit`)
- [ ] Trash behaviour of `deletefile` and `purge` in the Telia Sky UI
- [ ] bisync against Jottacloud: modtime precision, MD5 checksums (`compare=size,modtime,checksum`?), and how edits from the Telia Sky phone app show up

## Open decisions

- **License:** GPL-3.0-or-later is proposed. MIT or Apache would make a future commercial collaboration simpler. This needs deciding before the first external contribution.
- **OAuth client:** rclone uses the `desktop` client id for `telia_no`. Shipping the same client in a separate product may need Telia's blessing, and is a topic for phase 6 outreach.
- **Branding:** we describe compatibility ("for Telia Sky") and never use Telia's logo or styling.

## Real-account run with Sky Linux itself (5 November 2026)

Sky Linux 0.1.0 (the installed `.deb`, rclone v1.75.2) against a real Telia Sky account, signed in with Sky Linux's own sign-in. The account holds 60.8 GiB (4,250 files), all in Photos. Write tests ran in a scratch folder in Archive, which was deleted afterwards.

| Check | Result |
| --- | --- |
| Sign-in (authorization code + PKCE), all four areas found | ✅ |
| Areas listed through the combined remote match each mountpoint listed on its own | ✅ after the fix below; ❌ before |
| Storage used | ✅ 65.3 GB after the fix below; 261 GB (4×) before. The plan reports no total or free space; the UI shows "no storage limit". |
| Upload/download (12 MB + small file), sha256 identical; second download becomes `name (1).ext` | ✅ |
| Files landed in Archive, checked with rclone outside Sky Linux; nothing in Photos | ✅ |
| Create folder, move + rename, delete; writing to Photos is refused | ✅ |
| Backup is upload-only (a deleted local file stays in the cloud) | ✅ |
| Two-way sync: up, down, deletion both ways, recovery folder, conflict (newer wins, other kept as a numbered copy) | ✅ One bisync run aborted once and succeeded on the automatic retry 3 seconds later; the cause wasn't captured. |
| Token renewal: expired token renewed on use, the new refresh token saved, still valid after a restart, no `invalid_grant` across areas | ✅ |

**Bug found and fixed: areas could be mixed up.** rclone caches backends by a name built from the *registered* options a connection string overrides. `mountpoint` isn't one for jottacloud, so `sky-telia-no,mountpoint=Archive:` and `sky-telia-no,mountpoint=Photos:` shared one cache entry. Within one rclone process, "Archive" randomly listed Photos' content (seen in 3 of 5 runs). Writes would have been just as unpredictable. Each area's connection string now also sets `description=<area>`, a registered option, which gives every area its own cache entry (5 of 5 runs correct). Existing installs are repaired when the daemon starts. No writes were made before the fix.

**Bug found and fixed: storage counted once per area.** Every area reports the whole account's usage, so `about` on the combined remote returned 4× the real figure. Usage is now read from the signed-in remote.
