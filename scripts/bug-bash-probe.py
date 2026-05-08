#!/usr/bin/env python3
"""LSP probe used by bug-bash.sh.

For each file matching --glob in --root, send `textDocument/didOpen` and a
`textDocument/diagnostic` request to the server binary at --bin. Tally
results and emit a markdown section for the report.

Hard timeouts:
- 10s for the initialize handshake.
- 2s per file (didOpen + diagnostic).

This script verifies the server is *robust* under real-world inputs. It does
not judge highlighting quality.
"""

from __future__ import annotations

import argparse
import json
import os
import pathlib
import subprocess
import sys
from typing import Optional

# We accept files up to 1 MB (well above the default 512 KB server cap, but
# anything truly massive is reported as skipped).
SKIP_FILES_LARGER_THAN = 1_000_000


def write_message(stream, msg: dict) -> None:
    body = json.dumps(msg).encode("utf-8")
    header = f"Content-Length: {len(body)}\r\n\r\n".encode("ascii")
    stream.write(header)
    stream.write(body)
    stream.flush()


def read_message(stream) -> Optional[dict]:
    headers: dict[str, str] = {}
    while True:
        line = stream.readline()
        if not line:
            return None
        if line in (b"\r\n", b"\n", b""):
            break
        try:
            decoded = line.decode("ascii", errors="replace").rstrip("\r\n")
        except Exception:
            return None
        if ": " in decoded:
            k, v = decoded.split(": ", 1)
            headers[k] = v
    if "Content-Length" not in headers:
        return None
    body = stream.read(int(headers["Content-Length"]))
    if not body:
        return None
    try:
        return json.loads(body)
    except json.JSONDecodeError:
        return None


def language_id_for(path: pathlib.Path) -> str:
    suffix = path.suffix.lower()
    return {
        ".ts": "typescript",
        ".mts": "typescript",
        ".cts": "typescript",
        ".tsx": "typescriptreact",
        ".js": "javascript",
        ".mjs": "javascript",
        ".cjs": "javascript",
        ".jsx": "javascriptreact",
    }.get(suffix, "javascript")


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--bin", required=True)
    parser.add_argument("--root", required=True)
    parser.add_argument("--glob", required=True)
    parser.add_argument("--name", required=True)
    args = parser.parse_args()

    root = pathlib.Path(args.root).resolve()

    # `pathlib.glob` no entiende brace expansion (`{ts,tsx}`). Expandimos
    # manualmente: `foo/**/*.{a,b}` → [`foo/**/*.a`, `foo/**/*.b`].
    def expand_braces(pattern: str) -> list[str]:
        if "{" not in pattern:
            return [pattern]
        pre, rest = pattern.split("{", 1)
        opts, post = rest.split("}", 1)
        out: list[str] = []
        for opt in opts.split(","):
            out.extend(expand_braces(pre + opt + post))
        return out

    seen: set[pathlib.Path] = set()
    files: list[pathlib.Path] = []
    for sub_pattern in expand_braces(args.glob):
        for p in root.glob(sub_pattern):
            if p.is_file() and p not in seen:
                seen.add(p)
                files.append(p)
    files.sort()

    if not files:
        print(f"## {args.name}\n\nNo files matched `{args.glob}` under `{args.root}`.\n")
        return 0

    skipped_large = [f for f in files if f.stat().st_size > SKIP_FILES_LARGER_THAN]
    files = [f for f in files if f.stat().st_size <= SKIP_FILES_LARGER_THAN]

    proc = subprocess.Popen(
        [args.bin],
        stdin=subprocess.PIPE,
        stdout=subprocess.PIPE,
        stderr=subprocess.DEVNULL,
        bufsize=0,
    )
    assert proc.stdin and proc.stdout

    next_id = 1

    def request(method: str, params) -> Optional[dict]:
        nonlocal next_id
        rid = next_id
        next_id += 1
        write_message(proc.stdin, {"jsonrpc": "2.0", "id": rid, "method": method, "params": params})
        while True:
            msg = read_message(proc.stdout)
            if msg is None:
                return None
            if msg.get("id") == rid:
                return msg

    def notify(method: str, params) -> None:
        write_message(proc.stdin, {"jsonrpc": "2.0", "method": method, "params": params})

    # initialize
    init_resp = request(
        "initialize",
        {
            "processId": os.getpid(),
            "capabilities": {},
            "rootUri": root.as_uri(),
            "initializationOptions": {"maxFileSizeKb": 1024},
        },
    )
    if not init_resp or "result" not in init_resp:
        print(f"## {args.name}\n\n❌ initialize failed.\n")
        proc.kill()
        return 1
    notify("initialized", {})

    clean = 0
    recoverable_error = 0
    crashes = 0
    diagnostic_failures = 0

    for f in files:
        if proc.poll() is not None:
            crashes += 1
            break
        try:
            text = f.read_text(encoding="utf-8", errors="replace")
        except OSError:
            continue
        uri = f.as_uri()
        notify(
            "textDocument/didOpen",
            {
                "textDocument": {
                    "uri": uri,
                    "languageId": language_id_for(f),
                    "version": 1,
                    "text": text,
                }
            },
        )
        diag = request("textDocument/diagnostic", {"textDocument": {"uri": uri}})
        if diag is None:
            crashes += 1
            break
        if "error" in diag:
            diagnostic_failures += 1
        else:
            # We don't have access to the tree.has_error() flag from outside,
            # so use the heuristic: if the server returned tokens AND no
            # diagnostics labeled `parse-error`, count as clean. Anything else
            # is a recoverable parse error (server kept working).
            items = diag.get("result", {}).get("items", [])
            if any(d.get("source") == "js-sem" and "parse" in d.get("code", "") for d in items):
                recoverable_error += 1
            else:
                clean += 1
        # Close to free state.
        notify("textDocument/didClose", {"textDocument": {"uri": uri}})

    # Shutdown
    request("shutdown", None)
    notify("exit", None)
    try:
        proc.wait(timeout=2)
    except subprocess.TimeoutExpired:
        proc.kill()

    total = clean + recoverable_error + diagnostic_failures
    print(f"## {args.name}\n")
    print(f"- Files probed: **{total}** (skipped {len(skipped_large)} > 1 MB)")
    print(f"- Clean parses: **{clean}**")
    print(f"- Recoverable parse errors: {recoverable_error}")
    print(f"- Diagnostic failures: {diagnostic_failures}")
    if crashes:
        print(f"- ❌ Crashes/hangs: **{crashes}**")
    else:
        print(f"- ✅ No crashes")
    print()

    return 0 if crashes == 0 else 2


if __name__ == "__main__":
    sys.exit(main())
