#!/bin/bash
# RI1 Share — start sharing RI1 with a link.  Ctrl-C stops everything.
#   ./share.sh
set -u
cd "$(dirname "$0")"

MODEL="${RI1_SHARE_MODEL:-ri1-llama:desktop}"
PORT="${RI1_SHARE_PORT:-8787}"
export RI1_SHARE_MODEL="$MODEL" RI1_SHARE_PORT="$PORT"
export RI1_SHARE_PASSCODE="${RI1_SHARE_PASSCODE:-$(openssl rand -hex 2)-$(openssl rand -hex 2)-$(openssl rand -hex 2)}"

say() { printf '%s\n' "$*"; }

# --- checks -------------------------------------------------------------------
command -v python3 >/dev/null || { say "python3 not found."; exit 1; }
if ! command -v cloudflared >/dev/null; then
  say "cloudflared (makes the share link) is not installed. Install it once with:"
  say "    brew install cloudflared"
  exit 1
fi
if ! curl -s --max-time 3 http://127.0.0.1:11434/api/tags >/dev/null; then
  say "Ollama isn't running. Open the Ollama app, then run this again."
  exit 1
fi
if ! curl -s http://127.0.0.1:11434/api/tags | grep -q "\"$MODEL\""; then
  say "Model $MODEL isn't in Ollama. Create it with:"
  say "    ollama create $MODEL -f ~/ri1-models/Modelfile.ri1-llama-desktop"
  exit 1
fi

LOG="$(mktemp -t ri1share)"
cleanup() {
  say ""; say "Stopping sharing — the link no longer works."
  kill "$SERVER_PID" "$TUNNEL_PID" "$AWAKE_PID" 2>/dev/null
  rm -f "$LOG"
  exit 0
}
trap cleanup INT TERM

# --- start --------------------------------------------------------------------
python3 server.py & SERVER_PID=$!
sleep 1
kill -0 "$SERVER_PID" 2>/dev/null || { say "The server failed to start (is port $PORT in use?)."; exit 1; }

# keep the Mac awake while sharing (display may dim; system won't sleep)
caffeinate -ims -w "$SERVER_PID" & AWAKE_PID=$!

cloudflared tunnel --no-autoupdate --url "http://127.0.0.1:$PORT" >"$LOG" 2>&1 & TUNNEL_PID=$!

URL=""
for _ in $(seq 1 40); do
  URL="$(grep -Eo 'https://[a-z0-9-]+\.trycloudflare\.com' "$LOG" | head -1)"
  [ -n "$URL" ] && break
  sleep 0.5
done

if [ -z "$URL" ]; then
  say "Couldn't create the share link. cloudflared said:"; tail -20 "$LOG"
  cleanup
fi

say "  ------------------------------------------------------------"
say "  Send your friend:"
say "    Link:      $URL"
say "    Passcode:  $RI1_SHARE_PASSCODE"
say "  (Send the passcode in a separate message from the link.)"
say ""
say "  Your Mac must stay on and awake with Ollama running."
say "  Press Ctrl-C here to stop sharing — the link dies immediately."
say "  ------------------------------------------------------------"
wait "$SERVER_PID"
cleanup
