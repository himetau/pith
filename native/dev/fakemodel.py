#!/usr/bin/env python3
# a stand-in for an openai-compatible chat completions endpoint, so pith can be driven end to end with no real model.
# scripted by the last user message:
#   contains "preview" -> the preview tool shows the path in the message
#   contains "browse" -> the browser tool navigates to the url in the message
#   contains "tool"  -> a bash tool call (ls), then a markdown summary once the result comes back
#   contains "long"  -> a long markdown reply with a code block and a table, streamed slowly
#   contains "slow"  -> a bash call that prints a line every half second, for the live tail and the working line
#   contains "edit"  -> an edit of notes.txt (run.sh creates it), so pi sends back a real diff
#   contains "read"  -> a read of notes.txt
#   contains "write" -> a write of hello.py
#   contains "ponder" -> a long stretch of reasoning streamed slowly, then a short reply
#   anything else    -> reasoning, then a short markdown reply
import json, sys, time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

PORT = int(sys.argv[1]) if len(sys.argv) > 1 else 18734

LONG = """## What I found

The session list is read on the **main thread**, which blocks the first frame. Moving it to a worker fixes the stall:

```rust
let list = cx.background_spawn(async { sessions::list_sessions() }).await;
```

| step | before | after |
|---|---|---|
| first frame | 310 ms | 18 ms |
| open session | 900 ms | 40 ms |

1. Read the file off disk first
2. Start pi behind it
3. Swap in pi's entries once it's up

That's it — `cargo build` and run it again.
"""

SCRIPTED = [
    ("slow", "bash", {"command": "for i in $(seq 1 12); do echo \"step $i of 12\"; sleep 0.5; done"}),
    ("edit", "edit", {"path": "notes.txt", "edits": [{"oldText": "hello", "newText": "hello\nworld, from the fake model"}]}),
    ("read", "read", {"path": "notes.txt"}),
    ("write", "write", {"path": "hello.py", "content": "import sys\n\n\ndef main():\n    # say hello to whoever is there\n    print(f\"hello {sys.argv[1:] or 'world'}\")\n\n\nif __name__ == \"__main__\":\n    main()\n"}),
]

PONDER = " ".join(["The user wants a careful answer, so I should weigh the options one by one before replying."] * 12)

def chunks(text, n=6):
    words = text.split(" ")
    for i in range(0, len(words), n):
        yield " ".join(words[i:i + n]) + (" " if i + n < len(words) else "")

class H(BaseHTTPRequestHandler):
    def log_message(self, *a): pass

    def do_GET(self):
        if self.path.endswith("/models"):
            body = json.dumps({"data": [{"id": "fake-1", "object": "model"}]}).encode()
            self.send_response(200); self.send_header("content-type", "application/json"); self.send_header("content-length", str(len(body))); self.end_headers(); self.wfile.write(body)
        else:
            self.send_response(404); self.end_headers()

    def send(self, delta, finish=None):
        ev = {"id": "c1", "object": "chat.completion.chunk", "created": int(time.time()), "model": "fake-1",
              "choices": [{"index": 0, "delta": delta, "finish_reason": finish}]}
        self.wfile.write(f"data: {json.dumps(ev)}\n\n".encode()); self.wfile.flush()

    def do_POST(self):
        n = int(self.headers.get("content-length", 0))
        req = json.loads(self.rfile.read(n) or b"{}")
        msgs = req.get("messages", [])
        last_user = next((m for m in reversed(msgs) if m.get("role") == "user"), {})
        text = last_user.get("content", "")
        if isinstance(text, list): text = " ".join(p.get("text", "") for p in text if isinstance(p, dict))
        after_tool = bool(msgs) and msgs[-1].get("role") == "tool"
        self.send_response(200)
        self.send_header("content-type", "text/event-stream"); self.send_header("cache-control", "no-cache"); self.end_headers()
        slow = 0.12 if "long" in text else 0.05
        if "preview" in text and not after_tool:
            # "preview <path>" shows a file in the preview pane
            path = next((w for w in text.split() if w.startswith("/")), "index.html")
            self.send({"tool_calls": [{"index": 0, "id": "call_p", "type": "function", "function": {"name": "preview", "arguments": json.dumps({"path": path})}}]})
            self.send({}, "tool_calls")
        elif "browse" in text and not after_tool:
            # "browse <url>" drives the browser tool
            url = next((w for w in text.split() if "://" in w or w.startswith("/")), "about:blank")
            self.send({"tool_calls": [{"index": 0, "id": "call_b", "type": "function", "function": {"name": "browser", "arguments": json.dumps({"action": "navigate", "url": url})}}]})
            self.send({}, "tool_calls")
        elif (scripted := next(((n, a) for k, n, a in SCRIPTED if k in text), None)) and not after_tool:
            name, args = scripted
            for c in chunks("On it.", 3):
                self.send({"content": c}); time.sleep(slow)
            # ids have to differ between turns, pi keys tool calls by them
            self.send({"tool_calls": [{"index": 0, "id": f"call_{time.time_ns()}", "type": "function", "function": {"name": name, "arguments": ""}}]})
            a = json.dumps(args)
            for i in range(0, len(a), 12):
                self.send({"tool_calls": [{"index": 0, "function": {"arguments": a[i:i + 12]}}]}); time.sleep(slow)
            self.send({}, "tool_calls")
        elif "ponder" in text and not after_tool:
            for c in chunks(PONDER, 3):
                self.send({"reasoning_content": c}); time.sleep(0.08)
            for c in chunks("Done thinking. The short answer is **yes**."):
                self.send({"content": c}); time.sleep(slow)
            self.send({}, "stop")
        elif "tool" in text and not after_tool:
            for c in chunks("Let me look at the folder first.", 3):
                self.send({"content": c}); time.sleep(slow)
            self.send({"tool_calls": [{"index": 0, "id": f"call_{time.time_ns()}", "type": "function", "function": {"name": "bash", "arguments": ""}}]})
            for part in ['{"command": ', '"ls -la"', '}']:
                self.send({"tool_calls": [{"index": 0, "function": {"arguments": part}}]}); time.sleep(slow)
            self.send({}, "tool_calls")
        else:
            for c in chunks("Thinking about how to answer this well and briefly.", 3):
                self.send({"reasoning_content": c}); time.sleep(slow)
            body = LONG if "long" in text else ("The folder has the usual files. **Nothing unusual** — the `src/` tree is where the work is." if after_tool else "Hello! I'm a *fake* model. Ask me for something `long`, or to use a `tool`.")
            for c in chunks(body):
                self.send({"content": c}); time.sleep(slow)
            self.send({}, "stop")
        usage = {"id": "c1", "object": "chat.completion.chunk", "choices": [], "usage": {"prompt_tokens": 1200, "completion_tokens": 80, "total_tokens": 1280}}
        self.wfile.write(f"data: {json.dumps(usage)}\n\n".encode())
        self.wfile.write(b"data: [DONE]\n\n"); self.wfile.flush()

ThreadingHTTPServer(("127.0.0.1", PORT), H).serve_forever()
