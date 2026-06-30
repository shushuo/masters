#!/usr/bin/env bash
# Run Masters as a headless web app (e.g. on WSL): start the getmastersd daemon, capture its
# handshake (port + per-launch token), then launch the Vite dev server wired to it.
#
# Open the printed URL in a browser on the host (Windows). Ctrl-C stops both.
set -euo pipefail
cd "$(dirname "$0")/.."

# Keep DB/state inside the repo instead of ~/.getmasters.
export GETMASTERS_DB_PATH="${GETMASTERS_DB_PATH:-$PWD/getmasters.db}"
# Allow the browser (Vite origin) to call the daemon cross-origin.
export GETMASTERS_DEV_CORS=1

echo "Building + starting getmastersd…"
fifo="$(mktemp -u)"; mkfifo "$fifo"
cargo run -q -p getmasters-server --bin getmastersd >"$fifo" &
daemon_pid=$!
trap 'kill "$daemon_pid" 2>/dev/null || true; rm -f "$fifo"' EXIT

# Wait for the handshake line, then keep draining stdout so the daemon never blocks on a full pipe.
ready=""
while IFS= read -r line; do
  echo "$line"
  if [[ "$line" == GETMASTERSD_READY* ]]; then ready="${line#GETMASTERSD_READY }"; break; fi
done <"$fifo"
cat "$fifo" & # drain the rest in the background

port="$(echo "$ready" | sed -E 's/.*"port":([0-9]+).*/\1/')"
token="$(echo "$ready" | sed -E 's/.*"token":"([^"]+)".*/\1/')"
echo "daemon ready on 127.0.0.1:$port"

cd ui/desktop
[[ -d node_modules ]] || pnpm install
# --host so the dev server is reachable from the Windows host; default port 1420.
VITE_GETMASTERS_PORT="$port" VITE_GETMASTERS_TOKEN="$token" pnpm dev -- --host
