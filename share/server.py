"""RI1 Share — let a friend chat with RI1 Desktop's model from a browser.

Runs on Andrew's Mac. The friend's browser talks ONLY to this server; this
server talks to Ollama on 127.0.0.1 with a fixed model and fixed settings.
Ollama itself is never exposed (its API can download and delete models).

Security model:
- every API call needs the passcode (printed at startup, never in the link);
- repeated wrong passcodes lock the server for 15 minutes;
- chat only: no Workshop, no files, no model management;
- one reply at a time; message and history sizes are capped;
- binds to 127.0.0.1, so it is only reachable through the tunnel you start.

Standard library only:  python3 server.py
"""
from __future__ import annotations

import hmac
import json
import os
import secrets
import sys
import threading
import time
import urllib.error
import urllib.request
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path

from basin_search import Basin

HERE = Path(__file__).resolve().parent
PORT = int(os.environ.get("RI1_SHARE_PORT", "8787"))
MODEL = os.environ.get("RI1_SHARE_MODEL", "ri1-llama:desktop")
OLLAMA = os.environ.get("OLLAMA_URL", "http://127.0.0.1:11434")
BASIN_DIR = os.environ.get("RI1_BASIN_PATH", str(Path.home() / "Desktop" / "RI1-BASIN" / "basin"))
PASSCODE = os.environ.get("RI1_SHARE_PASSCODE") or "-".join(secrets.token_hex(2) for _ in range(3))

MAX_MESSAGE = 2000        # characters in one message
MAX_HISTORY = 8           # earlier turns sent back to the model
MAX_HISTORY_CHARS = 12000
OPTIONS = {"num_predict": 768, "num_ctx": 8192}

BASIN_RULES = (
    "Use these passages where they bear on the request and cite them by number. If they do not "
    "contain the answer, say the provided documents don't state it, then answer from general "
    "knowledge and label it as general knowledge. Do not invent details the passages do not contain."
)

basin = Basin(BASIN_DIR) if Path(BASIN_DIR, "segments.jsonl").exists() else None
busy = threading.Lock()                 # one generation at a time
fail_lock = threading.Lock()
failures: list[float] = []
locked_until = 0.0


def log(msg: str) -> None:
    print(time.strftime("%H:%M:%S"), msg, flush=True)


def title_of(h: dict) -> str:
    name = Path(h["doc_path"]).stem
    for pre in ("RI1_", "RI1 ", "RI1-"):
        if name.upper().startswith(pre):
            name = name[len(pre):]
    name = name.strip()
    if name.endswith(")") and "(" in name and name[name.rfind("(") + 1:-1].isdigit():
        name = name[: name.rfind("(")].strip()
    return name or h["doc_id"]


def build_user_content(message: str, hits: list[dict]) -> str:
    if not hits:
        return message
    blocks = [f"[B{i + 1}] {title_of(h)} — page {h['page']}" + (f" — {h['heading']}" if h["heading"] else "")
              + f"\n{h['text']}" for i, h in enumerate(hits)]
    return "\n\n".join(["RI1 Basin passages (verbatim from Andrew's RI1 documents; cite them as [B1], [B2] …):",
                        *blocks, BASIN_RULES, "Request:\n" + message])


class Handler(BaseHTTPRequestHandler):
    server_version = "RI1Share/1"

    def log_message(self, fmt, *args):  # quiet default access log
        pass

    # ------------------------------------------------------------ helpers
    def _send(self, code: int, body: bytes, ctype: str = "application/json; charset=utf-8"):
        self.send_response(code)
        self.send_header("Content-Type", ctype)
        self.send_header("Content-Length", str(len(body)))
        self.send_header("Cache-Control", "no-store")
        self.send_header("X-Content-Type-Options", "nosniff")
        self.send_header("Referrer-Policy", "no-referrer")
        self.end_headers()
        self.wfile.write(body)

    def _json(self, code: int, obj) -> None:
        self._send(code, json.dumps(obj, ensure_ascii=False).encode())

    def _authorized(self) -> bool:
        global locked_until
        now = time.time()
        with fail_lock:
            if now < locked_until:
                return False
            given = self.headers.get("X-Passcode", "")
            if hmac.compare_digest(given.encode(), PASSCODE.encode()):
                return True
            failures.append(now)
            recent = [t for t in failures if now - t < 600]
            failures[:] = recent
            if len(recent) >= 10:
                locked_until = now + 900
                log("too many wrong passcodes — locked for 15 minutes")
            return False

    def _read_json(self, limit: int = 200_000):
        n = int(self.headers.get("Content-Length") or 0)
        if n <= 0 or n > limit:
            return None
        try:
            return json.loads(self.rfile.read(n))
        except (json.JSONDecodeError, UnicodeDecodeError):
            return None

    # ------------------------------------------------------------ routes
    def do_GET(self):
        if self.path in ("/", "/index.html"):
            self._send(200, (HERE / "index.html").read_bytes(), "text/html; charset=utf-8")
        elif self.path == "/api/check":
            if not self._authorized():
                return self._json(401, {"error": "Wrong passcode."})
            t1, t2 = basin.counts() if basin else (0, 0)
            self._json(200, {"model": MODEL, "basin": bool(basin), "curated": t1, "raw": t2})
        else:
            self._send(404, b"not found", "text/plain")

    def do_POST(self):
        if self.path != "/api/chat":
            return self._send(404, b"not found", "text/plain")
        if not self._authorized():
            return self._json(401, {"error": "Wrong passcode (or too many attempts — try again later)."})
        body = self._read_json()
        if not isinstance(body, dict) or not isinstance(body.get("message"), str):
            return self._json(400, {"error": "Bad request."})
        message = body["message"].strip()[:MAX_MESSAGE]
        if not message:
            return self._json(400, {"error": "Empty message."})
        use_basin = body.get("basin", True) is not False

        # earlier turns: only plain user/assistant text, newest kept, size-capped
        history = []
        for m in (body.get("history") or [])[-MAX_HISTORY:]:
            if isinstance(m, dict) and m.get("role") in ("user", "assistant") and isinstance(m.get("content"), str):
                history.append({"role": m["role"], "content": m["content"][:4000]})
        while history and sum(len(m["content"]) for m in history) > MAX_HISTORY_CHARS:
            history.pop(0)

        if not busy.acquire(blocking=False):
            return self._json(429, {"error": "RI1 is answering someone else right now — try again in a moment."})
        try:
            hits = basin.search(message) if (basin and use_basin) else []
            messages = history + [{"role": "user", "content": build_user_content(message, hits)}]
            log(f"question ({len(message)} chars, {len(hits)} Basin passages)")
            self._stream(messages, hits)
        finally:
            busy.release()

    def _stream(self, messages: list[dict], hits: list[dict]):
        """Server-sent events: one 'sources' event, then 'token' events, then 'done'."""
        self.send_response(200)
        self.send_header("Content-Type", "text/event-stream; charset=utf-8")
        self.send_header("Cache-Control", "no-store")
        self.send_header("X-Accel-Buffering", "no")
        self.end_headers()

        def event(kind: str, data) -> bool:
            try:
                self.wfile.write(f"event: {kind}\ndata: {json.dumps(data, ensure_ascii=False)}\n\n".encode())
                self.wfile.flush()
                return True
            except (BrokenPipeError, ConnectionResetError):
                return False

        sources = [{"n": i + 1, "title": title_of(h), "page": h["page"], "heading": h["heading"],
                    "tier": h["tier"], "text": h["text"]} for i, h in enumerate(hits)]
        if not event("sources", sources):
            return
        req = urllib.request.Request(
            f"{OLLAMA}/api/chat",
            data=json.dumps({"model": MODEL, "messages": messages, "stream": True, "options": OPTIONS}).encode(),
            headers={"Content-Type": "application/json"},
        )
        try:
            with urllib.request.urlopen(req, timeout=600) as resp:
                for line in resp:
                    if not line.strip():
                        continue
                    try:
                        v = json.loads(line)
                    except json.JSONDecodeError:
                        continue
                    if v.get("error"):
                        event("error", "The model reported an error.")
                        log(f"ollama error: {v['error']}")
                        return
                    piece = (v.get("message") or {}).get("content") or ""
                    if piece and not event("token", piece):
                        return  # browser went away; closing the response stops Ollama
                    if v.get("done"):
                        event("done", {"reason": v.get("done_reason", "")})
                        return
        except (urllib.error.URLError, TimeoutError, ConnectionError) as e:
            log(f"could not reach Ollama: {e}")
            event("error", "RI1's model isn't reachable right now (is Ollama running on the host Mac?).")


def main():
    if not (HERE / "index.html").exists():
        sys.exit("index.html missing next to server.py")
    srv = ThreadingHTTPServer(("127.0.0.1", PORT), Handler)
    t1, t2 = basin.counts() if basin else (0, 0)
    print("\n  RI1 Share is running")
    print(f"  model:    {MODEL}")
    print(f"  basin:    {BASIN_DIR}  ({t1} curated + {t2} raw passages)" if basin else
          f"  basin:    not found at {BASIN_DIR} — answers will have no sources")
    print(f"  local:    http://127.0.0.1:{PORT}")
    print(f"  PASSCODE: {PASSCODE}   (send this to your friend separately from the link)\n", flush=True)
    try:
        srv.serve_forever()
    except KeyboardInterrupt:
        pass


if __name__ == "__main__":
    main()
