"""RI1 Basin retrieval — Python twin of src-tauri/src/basin.rs.

Same rules as the desktop app: BM25 over curated segments (T1) first, raw
chunks (T2) as fallback; suspect / near-duplicate / demoted passages are
never served; reads the Basin read-only and reloads when it is rebuilt.
Standard library only.
"""
from __future__ import annotations

import json
import math
import os
import threading
import unicodedata
from dataclasses import dataclass, field
from pathlib import Path

STOP = set("""a an the and or of to in on for by with as at from is are was were be been it its this that
these those what which who whom how why when where do does did can could should would will about into
than then there their they them you your i me my we our us he she his her not no yes if so but also any
all some such has have had use used using between each per say says tell explain describe give list
according document documents""".split())

SUFFIXES = ["ations", "ation", "ions", "ion", "ings", "ing", "ies", "ied", "ers", "er", "ed", "es", "s"]


def _stem(t: str) -> str:
    if not t.isascii() or len(t) <= 4:
        return t
    for suf in SUFFIXES:
        if len(t) > len(suf) + 3 and t.endswith(suf):
            base = t[: -len(suf)]
            return base + "y" if suf in ("ies", "ied") else base
    return t


def _alnum(ch: str) -> bool:
    # mirrors Rust char::is_alphanumeric (Unicode letters + numbers)
    return unicodedata.category(ch)[0] in ("L", "N")


def tokenize(text: str) -> list[str]:
    out: list[str] = []
    cur: list[str] = []
    for ch in text:
        if _alnum(ch):
            cur.append(ch.lower())
            if not ch.isascii() and unicodedata.category(ch)[0] == "L":
                out.append(ch.lower())
        elif cur:
            out.append("".join(cur))
            cur = []
    if cur:
        out.append("".join(cur))
    out = [t for t in out if not (len(t) == 1 and t.isascii()) and t not in STOP]
    return [_stem(t) for t in out]


@dataclass
class Item:
    tier: str
    id: str
    doc_id: str
    doc_path: str
    page: int
    heading: str
    start: int
    end: int
    text: str
    pinned: bool
    tf: dict = field(default_factory=dict)
    length: float = 1.0


def _make_item(tier: str, v: dict) -> Item | None:
    if tier == "T1":
        if (v.get("selection") or {}).get("demoted") or (v.get("extraction") or {}).get("suspect"):
            return None
        doc, loc, sel = v.get("doc") or {}, v.get("locator") or {}, v.get("selection") or {}
        it = Item("T1", v.get("segment_id", ""), doc.get("doc_id", ""), doc.get("doc_path", ""),
                  int(loc.get("page") or 0), "", int(loc.get("char_start") or 0),
                  int(loc.get("char_end") or 0), v.get("text", ""), bool(sel.get("pinned")))
        heading_v = loc.get("heading_path")
    else:
        if v.get("suspect") or v.get("near_dup_of") is not None:
            return None
        it = Item("T2", v.get("chunk_id", ""), v.get("doc_id", ""), v.get("doc_path", ""),
                  int(v.get("page") or 0), "", int(v.get("char_start") or 0),
                  int(v.get("char_end") or 0), v.get("text", ""), False)
        heading_v = v.get("heading_path")
    if not it.text.strip():
        return None
    it.heading = " › ".join(h for h in (heading_v or []) if isinstance(h, str))
    title = Path(it.doc_path).stem.replace("_", " ").replace("-", " ")
    toks = tokenize(f"{it.heading} {it.text}")
    tt = tokenize(f"{title} {it.doc_id.removeprefix('ri1:').replace('-', ' ')}")
    toks += tt + tt
    tf: dict[str, int] = {}
    for t in toks:
        tf[t] = tf.get(t, 0) + 1
    it.tf, it.length = tf, float(max(1, len(toks)))
    return it


class _Tier:
    def __init__(self, items: list[Item]):
        self.items = items
        self.df: dict[str, int] = {}
        for it in items:
            for t in it.tf:
                self.df[t] = self.df.get(t, 0) + 1
        self.avgdl = (sum(i.length for i in items) / len(items)) if items else 1.0

    def score(self, q: list[str]) -> list[tuple[int, float]]:
        k1, b, n = 1.2, 0.75, float(len(self.items))
        need = len(q) if len(q) <= 2 else math.ceil(len(q) * 0.4)
        out = []
        for i, it in enumerate(self.items):
            s, matched = 0.0, 0
            for t in q:
                f = it.tf.get(t, 0)
                if not f:
                    continue
                matched += 1
                df = self.df.get(t, 0)
                idf = math.log((n - df + 0.5) / (df + 0.5) + 1.0)
                s += idf * f * (k1 + 1) / (f + k1 * (1 - b + b * it.length / self.avgdl))
            if matched >= need and s > 0:
                out.append((i, s * 1.15 if it.pinned else s))
        out.sort(key=lambda x: -x[1])  # stable, like Rust's sort_by
        return out


def _load_tier(path: Path, tier: str) -> _Tier:
    items = []
    with open(path, encoding="utf-8") as fh:
        for line in fh:
            line = line.strip()
            if not line:
                continue
            try:
                it = _make_item(tier, json.loads(line))
            except json.JSONDecodeError:
                continue
            if it:
                items.append(it)
    return _Tier(items)


def _truncate(text: str, n: int) -> str:
    text = text.strip()
    return text[:n] + " […]" if len(text) > n else text


class Basin:
    def __init__(self, directory: str | os.PathLike):
        self.dir = Path(directory)
        self._lock = threading.Lock()
        self._stamp = None
        self.t1 = self.t2 = None

    def _ensure(self):
        st = tuple((self.dir / f).stat().st_mtime if (self.dir / f).exists() else None
                   for f in ("segments.jsonl", "chunks.jsonl"))
        with self._lock:
            if self.t1 is None or st != self._stamp:
                self.t1 = _load_tier(self.dir / "segments.jsonl", "T1")
                self.t2 = _load_tier(self.dir / "chunks.jsonl", "T2")
                self._stamp = st

    def search(self, query: str, k: int = 6, char_budget: int = 6000) -> list[dict]:
        self._ensure()
        q, seen = [], set()
        for t in tokenize(query):
            if t not in seen:
                seen.add(t)
                q.append(t)
        if not q:
            return []
        per_hit, k_t1 = 1400, (k + 1) // 2
        hits, used, taken, texts = [], 0, [], set()

        def hit(it: Item, s: float) -> dict:
            return {"tier": it.tier, "id": it.id, "doc_id": it.doc_id, "doc_path": it.doc_path,
                    "page": it.page, "heading": it.heading, "text": _truncate(it.text, per_hit),
                    "score": round(s, 3)}

        r1 = self.t1.score(q)
        best1 = r1[0][1] if r1 else 0.0
        for i, s in r1:
            if len(hits) >= k_t1 or s < best1 * 0.35:
                break
            it = self.t1.items[i]
            h = hit(it, s)
            if used + len(h["text"].encode()) > char_budget:
                break
            used += len(h["text"].encode())
            taken.append((it.doc_id, it.page, it.start, it.end))
            texts.add(it.text.strip())
            hits.append(h)

        r2 = self.t2.score(q)
        best2 = r2[0][1] if r2 else 0.0
        for i, s in r2:
            if len(hits) >= k or s < best2 * 0.35:
                break
            it = self.t2.items[i]
            if any(d == it.doc_id and p == it.page and it.start < e and a < it.end for d, p, a, e in taken):
                continue
            if it.text.strip() in texts:
                continue
            texts.add(it.text.strip())
            h = hit(it, s)
            if used + len(h["text"].encode()) > char_budget:
                break
            used += len(h["text"].encode())
            taken.append((it.doc_id, it.page, it.start, it.end))
            hits.append(h)
        return hits

    def counts(self) -> tuple[int, int]:
        self._ensure()
        return len(self.t1.items), len(self.t2.items)
