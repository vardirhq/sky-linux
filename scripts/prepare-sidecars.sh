#!/usr/bin/env bash
# Build sky-daemon and fetch a pinned rclone, placing both where Tauri's
# `externalBin` expects them (binaries/<name>-<target-triple>).
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
RCLONE_VERSION="${RCLONE_VERSION:-v1.75.2}"
PROFILE="${PROFILE:-release}"
TRIPLE="${TARGET:-$(rustc -vV | sed -n 's/^host: //p')}"
OUT="$ROOT/apps/desktop/src-tauri/binaries"
mkdir -p "$OUT"

if [[ "$PROFILE" == "release" ]]; then
  cargo build --manifest-path "$ROOT/Cargo.toml" -p sky-daemon --release --target "$TRIPLE"
  cp "$ROOT/target/$TRIPLE/release/sky-daemon" "$OUT/sky-daemon-$TRIPLE"
else
  cargo build --manifest-path "$ROOT/Cargo.toml" -p sky-daemon
  cp "$ROOT/target/debug/sky-daemon" "$OUT/sky-daemon-$TRIPLE"
fi

case "$TRIPLE" in
  x86_64-*) ARCH=amd64 ;;
  aarch64-*) ARCH=arm64 ;;
  *) echo "unsupported target $TRIPLE" >&2; exit 1 ;;
esac
if [[ ! -x "$OUT/rclone-$TRIPLE" ]]; then
  TMP="$(mktemp -d)"
  trap 'rm -rf "$TMP"' EXIT
  NAME="rclone-$RCLONE_VERSION-linux-$ARCH"
  curl -fsSL -o "$TMP/r.zip" "https://downloads.rclone.org/$RCLONE_VERSION/$NAME.zip"
  curl -fsSL -o "$TMP/SHA256SUMS" "https://downloads.rclone.org/$RCLONE_VERSION/SHA256SUMS"
  (cd "$TMP" && grep " $NAME.zip\$" SHA256SUMS | sed "s/$NAME.zip/r.zip/" | sha256sum -c -)
  unzip -q -j "$TMP/r.zip" "$NAME/rclone" -d "$TMP"
  install -m 0755 "$TMP/rclone" "$OUT/rclone-$TRIPLE"
fi
# Packages install rclone privately to /usr/lib/sky-linux/rclone so it never
# conflicts with a distribution rclone package.
cp "$OUT/rclone-$TRIPLE" "$OUT/rclone"
echo "sidecars ready in $OUT"
