#!/usr/bin/env python3
"""
One-pass heuristic to backfill `axes` on every entry in tests/gold/questions.json.

The classifier is deliberately simple and conservative — run once, then
hand-review the output diff before committing. Borderline cases are
expected and should be corrected by hand.

Taxonomies:
- style: identifier | conceptual | mixed
- temporal_intent: historical | neutral | recency_seeking

Heuristics (first-match wins within each axis):

style
-----
- CVE/CWE id in question OR in must_contain_cve_ids (non-empty)  → identifier
- Named product / protocol / vuln class token in question
  (BlueKeep, Log4Shell, SMB, RDP, etc.) combined with a conceptual
  verb ("how", "why", "what does", "describe")                    → mixed
- Otherwise                                                       → conceptual

temporal_intent
---------------
- Question or notes mention "latest", "newest", "current", "recent",
  "zero-day", "being exploited"                                   → recency_seeking
- Question mentions an explicit past year (2014–2022) or notes
  contain "temporal-historical" or phrase "as of", "disclosed in"  → historical
- Otherwise                                                       → neutral
"""

from __future__ import annotations

import json
import re
import sys
from pathlib import Path

CVE_RE = re.compile(r"CVE-\d{4}-\d+", re.I)
CWE_RE = re.compile(r"CWE-\d+", re.I)
RECENCY_RE = re.compile(
    r"\b(latest|newest|current|recent|recently|zero[- ]?day|being exploited|this (week|month|quarter)|in 202[3-9])\b",
    re.I,
)
HISTORICAL_YEAR_RE = re.compile(r"\b(201[0-9]|2020|2021|2022)\b")
HISTORICAL_PHRASE_RE = re.compile(r"\b(disclosed|published|released|announced) in\b", re.I)
AS_OF_RE = re.compile(r"\bas of\b", re.I)

NAMED_THING_TOKENS = {
    "bluekeep", "log4shell", "spectre", "meltdown", "heartbleed",
    "shellshock", "printnightmare", "eternalblue", "wannacry",
    "spooler", "exchange", "struts", "solarwinds", "zerologon",
    "dirty cow", "smb", "rdp", "tls", "ssl", "dns", "http",
    "xml", "json", "xss", "ssrf", "csrf", "xxe", "idor",
}


def classify_style(entry: dict) -> str:
    question = entry["question"]
    if entry.get("must_contain_cve_ids"):
        return "identifier"
    if CVE_RE.search(question) or CWE_RE.search(question):
        return "identifier"

    q_lower = question.lower()
    named_present = any(tok in q_lower for tok in NAMED_THING_TOKENS)
    conceptual_verb_present = any(
        q_lower.startswith(v) or f" {v} " in q_lower
        for v in ("how ", "how do", "how does", "why ", "what does", "describe ", "explain ")
    )
    if named_present and conceptual_verb_present:
        return "mixed"
    return "conceptual"


def classify_temporal(entry: dict) -> str:
    question = entry["question"]
    notes = entry.get("notes") or ""
    blob = f"{question}\n{notes}"

    if "temporal-historical" in notes.lower():
        return "historical"
    if "temporal-recency" in notes.lower():
        return "recency_seeking"
    if RECENCY_RE.search(blob):
        return "recency_seeking"
    if HISTORICAL_YEAR_RE.search(question):
        return "historical"
    if HISTORICAL_PHRASE_RE.search(blob) or AS_OF_RE.search(blob):
        return "historical"
    return "neutral"


def main() -> int:
    path = Path("tests/gold/questions.json")
    if not path.exists():
        print(f"not found: {path}", file=sys.stderr)
        return 2

    data = json.loads(path.read_text())
    mutated = False
    style_hist: dict[str, int] = {}
    ti_hist: dict[str, int] = {}

    for entry in data["entries"]:
        axes = entry.get("axes")
        if axes is None:
            style = classify_style(entry)
            ti = classify_temporal(entry)
            entry["axes"] = {"style": style, "temporal_intent": ti}
            mutated = True
        else:
            style = axes["style"]
            ti = axes["temporal_intent"]
        style_hist[style] = style_hist.get(style, 0) + 1
        ti_hist[ti] = ti_hist.get(ti, 0) + 1

    if mutated:
        # Pretty-print to preserve diff-friendly format; 2-space indent matches existing file.
        path.write_text(json.dumps(data, indent=2, ensure_ascii=False) + "\n")

    print(f"entries: {len(data['entries'])}")
    print(f"style:          {style_hist}")
    print(f"temporal_intent:{ti_hist}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
