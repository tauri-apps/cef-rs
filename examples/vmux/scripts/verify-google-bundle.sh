#!/usr/bin/env bash
# Repro + gate: bundle vmux, launch with `open`, poll vmux-bevy.log for CEF proofs (~20s max).
# Required lines (plan): `proof: cef_navigated_url_has_google` and `proof: first_osr_frame_ready`.
#
# If the browser process traps in Chromium (e.g. EXC_BREAKPOINT / fontations on macOS 26 + CEF 146),
# capture ~/Library/Logs/DiagnosticReports/vmux*.ips and consider a CEF/Chromium bump (workspace download-cef).
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/../../.." && pwd)"
cd "$ROOT"
rm -rf target/bundle
cargo run -p cef --bin bundle-cef-app -- vmux -o target/bundle
rm -f target/bundle/vmux-bevy.log
open -n target/bundle/vmux.app
LOG="target/bundle/vmux-bevy.log"
for _ in $(seq 1 20); do
  sleep 1
  if [[ -f "$LOG" ]] && rg -q 'proof: cef_navigated_url_has_google' "$LOG" && rg -q 'proof: first_osr_frame_ready' "$LOG"; then
    echo "verify-google-bundle: PASS (both proof lines in $LOG)"
    exit 0
  fi
done
echo "verify-google-bundle: FAIL — proofs missing after 20s ($LOG)" >&2
[[ -f "$LOG" ]] && tail -40 "$LOG" >&2 || echo "(no log file)" >&2
if pgrep -fq 'vmux.app/Contents/MacOS/vmux'; then
  echo "verify-google-bundle: vmux still running (no proofs in log)" >&2
else
  echo "verify-google-bundle: vmux not running (likely Chromium trap — see DiagnosticReports)" >&2
fi
exit 1
