# Security model

## Secrets

- OAuth tokens live only in Sky Linux's **private** `~/.config/sky-linux/rclone.conf`. Your own `~/.config/rclone` is never read or written.
- That file is **encrypted** by rclone. The password is generated randomly on first start and kept in the Secret Service (GNOME Keyring, KWallet, KeePassXC). It reaches rclone only through the child's environment, never through argv.
- If no Secret Service is available, the daemon logs a warning and runs with an unencrypted config (mode 0600 in a 0700 directory).
- SQLite holds no secrets. The IPC `settings.*` methods only accept an allowlist of UI keys.

## Local attack surface

| Channel | Protection |
| --- | --- |
| rclone RC API | Unix socket in `$XDG_RUNTIME_DIR/sky-linux` (0700), plus random basic-auth credentials per launch. No TCP port and no web GUI. |
| Daemon IPC | Unix socket with mode 0600, and the peer UID is checked with `SO_PEERCRED`. A second daemon refuses to steal a live socket. |
| Webview | Strict CSP and Tauri capabilities limited to dialog, notification, autostart and opener. No shell or filesystem plugin, and no remote content. |
| Environment | rclone is started with a cleared environment plus an allowlist, so stray `RCLONE_*` variables can't change its behaviour. |

## Safety rules for user data

- Backups use `sync/copy`, which **never deletes** remote files. Backing up `/`, `/etc`, `/usr` and similar folders is refused.
- A backup whose source folder has disappeared (for example an unmounted drive) is skipped, never mirrored as deletions.
- Downloads never overwrite existing local files.
- Signing out or removing a backup job never deletes local or cloud files. Uninstalling the package leaves user data alone.
- Remote deletes are single files only and go to the provider's trash. Folder deletion waits for a trash-aware confirmation flow.
- Two-way sync stays off until phase 4's safeguards (`--max-delete`, check-access files, conflict copies, dry-run previews) are in place and tested.

## Reporting

Please report vulnerabilities privately via GitHub Security Advisories on this repository.
