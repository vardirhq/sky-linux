# Roadmap

| Phase | Scope | Status |
| --- | --- | --- |
| 0 Feasibility | Real Telia Sky auth, operations, token refresh, provider limits, licensing | Local findings in [FEASIBILITY.md](FEASIBILITY.md). **Next: run `scripts/feasibility.sh` with a real account.** |
| 1 Sky Core foundation | Typed models, rclone adapter, supervision, secure IPC, SQLite, structured events | ✅ Done, with unit and end-to-end tests |
| 2 Desktop MVP | Login, browsing, manual up/download, transfer progress, error handling | ✅ UI in place. Norwegian (default) and English done. Still to do: polish from real-account testing |
| 3 Background backups | Folder selection, upload-only jobs, file watching, schedules, restart recovery | ✅ Core done. Still to do: notifications for failed backups, bandwidth limits, pause on metered networks |
| 4 Two-way sync | Evaluate `rclone bisync`; conflicts, deletion protection, extensive integration tests | ✅ Core and UI done, 7 end-to-end safety tests. Still to do: test against real Jottacloud, a "restore from recovery folder" view, selective sync |
| 5 Distribution | Release CI for deb/rpm/AppImage on x86_64 + arm64, Flatpak validation, AUR | Configs drafted |
| 6 Public release | Docs site, community feedback, more providers, contact Telia/Jottacloud | — |

## Next concrete tasks

1. Run the feasibility script with a real Telia Sky account and record the report in this repo.
2. Fetch the user's name and email after sign-in (Jottacloud's user-info endpoint, or the `about` extras) for the greeting and avatar.
3. Desktop notifications for backup failures and for an expired sign-in.
4. Release workflow that builds and signs packages on tag.
5. More translations (sv-SE for Telia Cloud, da-DK); localize the demo data.
