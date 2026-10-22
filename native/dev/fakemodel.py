#!/usr/bin/env python3
# a stand-in for an openai-compatible chat completions endpoint, so pith can be driven end to end with no real model.
# scripted by the last user message:
#   contains "browse" -> the browser tool navigates to the url in the message
#   contains "tool"  -> a bash tool call (ls), then a markdown summary once the result comes back
#   contains "long"  -> a long markdown reply with a code block and a table, streamed slowly
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
        if "browse" in text and not after_tool:
            # "browse <url>" drives the browser tool
            url = next((w for w in text.split() if "://" in w or w.startswith("/")), "about:blank")
            self.send({"tool_calls": [{"index": 0, "id": "call_b", "type": "function", "function": {"name": "browser", "arguments": json.dumps({"action": "navigate", "url": url})}}]})
            self.send({}, "tool_calls")
        elif "tool" in text and not after_tool:
            for c in chunks("Let me look at the folder first.", 3):
                self.send({"content": c}); time.sleep(slow)
            self.send({"tool_calls": [{"index": 0, "id": "call_1", "type": "function", "function": {"name": "bash", "arguments": ""}}]})
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
