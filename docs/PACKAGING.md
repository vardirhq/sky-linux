# Packaging

One codebase produces several package formats. The binaries are `sky-linux` (UI), `sky-daemon` (service) and a bundled, checksum-verified `rclone`.

| Format | How | Status |
| --- | --- | --- |
| `.deb` / `.rpm` / AppImage | `scripts/prepare-sidecars.sh && cd apps/desktop && npm run tauri build` | Configured in `tauri.conf.json`. The packages install `/usr/bin/sky-daemon`, `/usr/lib/sky-linux/rclone` (private, so it doesn't conflict with a distro rclone) and the systemd user unit. |
| AUR | `packaging/aur/PKGBUILD` | Draft. Uses the system `rclone`. |
| Flatpak | `packaging/flatpak/io.github.vardirhq.SkyLinux.yml` | Experimental, see below. |

## Background service per format

- **deb/rpm/AUR:** `systemctl --user enable --now sky-daemon.service`. The app also starts the unit, or the binary directly, if it isn't running.
- **AppImage:** there's no system integration, so the app spawns `sky-daemon` itself. "Start on login" uses an XDG autostart entry through `tauri-plugin-autostart`.
- **Flatpak:** a sandboxed app can't install a systemd user unit. Open questions:
  - Use the Background portal (`org.freedesktop.portal.Background.RequestBackground`) to autostart the app with `--hidden`, and run the daemon as a child of the app process.
  - inotify on host paths needs `--filesystem=home` (or narrower), because portal document handles don't support recursive watching. Flathub reviewers may push back.
  - The Secret Service is reachable via `--talk-name=org.freedesktop.secrets`.

## Updates and uninstall

Updates come from the packaging system (apt/dnf/pacman/Flathub). There's no self-updater. Uninstalling removes program files only: `~/.config/sky-linux`, `~/.local/share/sky-linux` and all user files stay.

## Architectures

x86_64 and aarch64. `prepare-sidecars.sh` picks the matching rclone build from the target triple.
