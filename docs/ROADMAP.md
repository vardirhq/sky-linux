# Roadmap

| Phase | Scope | Status |
| --- | --- | --- |
| 0 Feasibility | Real Telia Sky auth, operations, token refresh, provider limits, licensing | ✅ Sign-in, all operations, areas, backups, sync and token renewal tested on a real account ([FEASIBILITY.md](FEASIBILITY.md)). Licensing still open |
| 1 Sky Core foundation | Typed models, rclone adapter, supervision, secure IPC, SQLite, structured events | ✅ Done, with unit and end-to-end tests |
| 2 Desktop MVP | Login, browsing, manual up/download, transfer progress, error handling | ✅ UI in place. Norwegian (default) and English done. Still to do: polish from real-account testing |
| 3 Background backups | Folder selection, upload-only jobs, file watching, schedules, restart recovery | ✅ Core done, plus start on login, notifications, bandwidth limit and a diagnostics report. Still to do: pause on metered networks |
| 4 Two-way sync | Evaluate `rclone bisync`; conflicts, deletion protection, extensive integration tests | ✅ Core and UI done, 7 end-to-end safety tests. Tested on a real account. Restoring from the recovery folder and selective sync are in the app. Still to do: subfolders deeper than the first level in selective sync |
| 5 Distribution | Release CI for deb/rpm/AppImage on x86_64 + arm64, Flatpak validation, AUR | deb/rpm/AppImage built, installed and run; release workflow (draft releases, glibc 2.35 check). Still to do: real-desktop test, arm64, Flatpak, AUR |
| 6 Public release | Docs site, community feedback, more providers, contact Telia/Jottacloud | — |

## Next concrete tasks

1. Check on a real account that Telia's sign-in includes the name and email claims (the app falls back to "Telia Sky" without them).
2. Fetch the user's name and email after sign-in (Jottacloud's user-info endpoint, or the `about` extras) for the greeting and avatar.
3. Test notifications and start on login in a real desktop session (GNOME, KDE).
4. Sign release packages.
5. More translations (sv-SE for Telia Cloud, da-DK); localize the demo data.
