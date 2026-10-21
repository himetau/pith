#!/usr/bin/env python3
# ctl.py js '<code>' | shot <file.png> [w h]
# sends one command to the app started by tauri-run.sh (or the electron build with PITH_CTL) and prints the answer
import json, os, sys, time, uuid
d = os.path.join(os.environ.get("PITH_DEV_DIR", "/tmp/pith-dev"), "ctl")
cid = uuid.uuid4().hex[:8]
cmd = {}
if sys.argv[1] == "js": cmd["js"] = sys.argv[2]
else:
    cmd["shot"] = os.path.abspath(sys.argv[2])
    if len(sys.argv) > 4: cmd["size"] = [int(sys.argv[3]), int(sys.argv[4])]
tmp = os.path.join(d, cid + ".tmp")
open(tmp, "w").write(json.dumps(cmd)); os.rename(tmp, os.path.join(d, cid + ".cmd.json"))
out = os.path.join(d, cid + ".out.json")
for _ in range(300):
    if os.path.exists(out):
        time.sleep(0.05); print(open(out).read()[:20000]); os.remove(out); break
    time.sleep(0.1)
else: print("timeout")
