#!/usr/bin/env bash
# Phase 0 feasibility check against a REAL Telia Sky account.
#
# Verifies, with an isolated rclone config (your own ~/.config/rclone is
# never touched):
#   1. browser sign-in via rclone's Jottacloud "telia_no" white-label
#   2. account/quota information
#   3. listing the root
#   4. upload → download → checksum compare → delete, in a scratch folder
#   5. token refresh: forces the access token to look expired and checks
#      that rclone silently obtains a new one and writes it back
#
# Usage:  scripts/feasibility.sh [--keep-config]
# Output: a Markdown report in ./feasibility-report-<date>.md
set -euo pipefail

RCLONE="${SKY_RCLONE:-rclone}"
WORK="$(mktemp -d -t sky-feasibility-XXXX)"
CONF="$WORK/rclone.conf"
REMOTE="sky-feasibility"
SCRATCH="sky-linux-feasibility-$(date +%s)"
REPORT="$PWD/feasibility-report-$(date +%F).md"
KEEP=0; [[ "${1:-}" == "--keep-config" ]] && KEEP=1
cleanup() { [[ $KEEP == 1 ]] && echo "config kept at $CONF" || rm -rf "$WORK"; }
trap cleanup EXIT

rc() { "$RCLONE" --config "$CONF" "$@"; }
pass() { echo "- [x] $*" | tee -a "$REPORT"; }
fail() { echo "- [ ] **FAILED:** $*" | tee -a "$REPORT"; }
step() { echo; echo "==> $*"; }

command -v "$RCLONE" >/dev/null || { echo "rclone not found (set SKY_RCLONE)"; exit 1; }
{
  echo "# Telia Sky feasibility report — $(date -Iseconds)"
  echo
  echo "rclone: \`$($RCLONE version | head -1)\`  "
  echo "host: \`$(uname -srm)\`"
  echo
} > "$REPORT"

step "1/5 Sign in (your browser will open Telia's sign-in page)"
# Drive the same non-interactive state machine Sky Core uses.
q=$(rc config create "$REMOTE" jottacloud --non-interactive)
answer() {
  case "$1" in
    config_type) echo traditional ;;
    config_traditional) echo telia_no ;;
    config_is_local) echo true ;;
    # Ask rclone to list the account's devices and mountpoints instead of
    # silently using the default (Jotta/Archive), which may be empty.
    config_non_standard) echo true ;;
    *) echo "__ask__" ;;
  esac
}
# Show a question with its choices and read the answer from the terminal.
ask() {
  python3 - "$1" <<'PY' >/dev/tty
import json, sys
q = json.loads(sys.argv[1]); o = q.get("Option") or {}
print(f"\n    {o.get('Name')}: {(o.get('Help') or '').splitlines()[0] if o.get('Help') else ''}")
for e in o.get("Examples") or []:
    print(f"      - {e.get('Value')!r}: {(e.get('Help') or '').splitlines()[0] if e.get('Help') else ''}")
d = o.get("Default")
print(f"    default: {str(d).lower() if isinstance(d, bool) else d!r}")
PY
  local def
  def=$(python3 -c 'import json,sys; o=json.load(sys.stdin)["Option"] or {}; d=o.get("Default"); print(str(d).lower() if isinstance(d,bool) else (d or ""))' <<<"$1")
  read -r -p "    choice [$def]: " reply </dev/tty
  echo "${reply:-$def}"
}
for _ in $(seq 1 20); do
  state=$(python3 -c 'import json,sys; print(json.load(sys.stdin)["State"])' <<<"$q")
  [[ -z "$state" ]] && break
  opt=$(python3 -c 'import json,sys; o=json.load(sys.stdin)["Option"] or {}; print(o.get("Name",""))' <<<"$q")
  if [[ "$opt" == config_mountpoint ]]; then
    # Remember every storage area on the device for step 3.
    python3 -c 'import json,sys; [print(e["Value"]) for e in json.load(sys.stdin)["Option"].get("Examples") or []]' <<<"$q" > "$WORK/mountpoints"
  fi
  ans=$(answer "$opt")
  [[ "$ans" == "__ask__" ]] && ans=$(ask "$q")
  echo "    $opt → $ans"
  [[ "$opt" == config_device || "$opt" == config_mountpoint ]] && echo "- Chose $opt = \`$ans\`" >> "$REPORT"
  q=$(rc config update "$REMOTE" --continue --state "$state" --result "$ans" --non-interactive)
done
if rc about "$REMOTE:" >/dev/null 2>&1; then pass "Browser sign-in with telia_no white-label"; else fail "sign-in"; exit 1; fi

step "2/5 Account information"
about=$(rc about "$REMOTE:" --json)
echo "$about"
pass "Quota: \`$(python3 -c '
import json, sys
d = json.load(sys.stdin)
used, total = d.get("used", 0) / 1e9, (d.get("total") or 0) / 1e9
print(f"{used:.1f} GB used of {total:.0f} GB" if total else f"{used:.1f} GB used, no total reported (unlimited plan?)")
' <<<"$about")\`"

step "3/5 Listing"
n=$(rc lsjson "$REMOTE:" | python3 -c 'import json,sys; print(len(json.load(sys.stdin)))')
echo "    top-level entries (shown here only, not written to the report):"
rc lsf "$REMOTE:" --max-depth 1 | head -20 | sed 's/^/      /'
if [[ "$n" == 0 ]]; then
  fail "Listed root: 0 entries — your files are probably in another device/mountpoint; re-run and pick a different one"
else
  pass "Listed root: $n entries"
fi

# Every storage area through the same remote (and token), using an rclone
# connection string: "remote,mountpoint=Photos:". Counts and sizes only.
[[ -s "$WORK/mountpoints" ]] || printf 'Archive\nPhotos\nShared\nSync\n' > "$WORK/mountpoints"
echo "    storage areas:"
while read -r mp; do
  [[ -z "$mp" ]] && continue
  area="$REMOTE,mountpoint=$mp:"
  if top=$(rc lsjson "$area" 2>"$WORK/err"); then
    count=$(python3 -c 'import json,sys; print(len(json.load(sys.stdin)))' <<<"$top")
    size=$(timeout 300 "$RCLONE" --config "$CONF" size "$area" --json 2>/dev/null \
      | python3 -c 'import json,sys; d=json.load(sys.stdin); c, b = d["count"], d["bytes"] / 1e9; print(f"{c} files, {b:.1f} GB")' 2>/dev/null || echo "size unavailable")
    echo "      $mp: $count top-level entries, $size"
    pass "Storage area \`$mp\`: $count top-level entries, $size"
  else
    echo "      $mp: error: $(tail -1 "$WORK/err")"
    fail "Storage area \`$mp\`: $(tail -1 "$WORK/err" | cut -c1-200)"
  fi
done < "$WORK/mountpoints"

step "4/5 Upload / download round trip in $SCRATCH"
head -c 5000000 /dev/urandom > "$WORK/probe.bin"
printf 'Sky Linux feasibility ✓ æøå\n' > "$WORK/unicode æøå.txt"
t0=$(date +%s.%N)
rc copy "$WORK/probe.bin" "$REMOTE:$SCRATCH/" && rc copy "$WORK/unicode æøå.txt" "$REMOTE:$SCRATCH/"
t1=$(date +%s.%N)
rc copy "$REMOTE:$SCRATCH/" "$WORK/down/"
if cmp -s "$WORK/probe.bin" "$WORK/down/probe.bin" && cmp -s "$WORK/unicode æøå.txt" "$WORK/down/unicode æøå.txt"; then
  pass "Upload + download byte-identical (5 MB in $(python3 -c "print(f'{$t1-$t0:.1f}')")s), unicode names preserved"
else
  fail "round trip mismatch"
fi
rc check "$WORK" "$REMOTE:$SCRATCH" --one-way --exclude 'down/**' --exclude rclone.conf >/dev/null 2>&1 \
  && pass "Server-side MD5 checksums match local files" || fail "checksum check"
rc purge "$REMOTE:$SCRATCH" && pass "Scratch folder removed (goes to Telia Sky trash)"

step "5/5 Token refresh"
before=$(rc config dump | python3 -c "import json,sys; t=json.loads(json.load(sys.stdin)['$REMOTE']['token']); print(t['access_token'][-12:], t['expiry'])")
echo "    before: $before"
# Make the token look expired; rclone must refresh it using the refresh token.
python3 - "$CONF" "$REMOTE" <<'PY'
import configparser, json, sys
path, remote = sys.argv[1:]
cp = configparser.RawConfigParser()
cp.optionxform = str  # keep key case: rclone needs "configVersion", not "configversion"
cp.read(path)
tok = json.loads(cp[remote]["token"]); tok["expiry"] = "2000-01-01T00:00:00Z"
cp[remote]["token"] = json.dumps(tok)
with open(path, "w") as f: cp.write(f)
PY
rc lsd "$REMOTE:" >/dev/null
after=$(rc config dump | python3 -c "import json,sys; t=json.loads(json.load(sys.stdin)['$REMOTE']['token']); print(t['access_token'][-12:], t['expiry'])")
echo "    after:  $after"
if [[ "$before" != "$after" && "$after" != *2000-01-01* ]]; then
  pass "Expired access token refreshed automatically and persisted"
else
  fail "token refresh (before: $before, after: $after)"
fi

{
  echo
  echo "## Notes to verify by hand"
  echo
  echo "- Does signing in here sign out Telia's Windows/macOS client or the mobile apps? (rclone warns that"
  echo "  traditional auth allows only a single active authentication.)"
  echo "- How long does the refresh token live without use? Re-run step 5 after a week."
  echo "- Where do uploaded files appear in the Telia Sky web/app UI (device \`Jotta\`, mount \`Archive\`)?"
} >> "$REPORT"
echo
echo "Report written to $REPORT"
