#!/usr/bin/env python3
# ctl.py [name] '<json>' ['<json>' ...]: sends each command to the app native/dev/run.sh started, prints each answer.
# the commands are in src/dev.rs: keys, type, click, move, scroll, open, view, resize, dump
import json, os, sys, time, uuid
args = sys.argv[1:]
name = "dev"
if args and not args[0].lstrip().startswith("{"): name, args = args[0], args[1:]
d = os.path.join(os.environ.get("PITH_DEV_DIR", "/tmp/pith-native-dev"), name, "ctl")
for arg in args:
    json.loads(arg)
    cid = f"{time.time_ns()}-{uuid.uuid4().hex[:6]}"
    tmp = os.path.join(d, cid + ".tmp")
    open(tmp, "w").write(arg)
    os.rename(tmp, os.path.join(d, cid + ".cmd.json"))
    out = os.path.join(d, cid + ".out.json")
    for _ in range(200):
        if os.path.exists(out):
            time.sleep(0.01); print(open(out).read()[:40000]); os.remove(out); break
        time.sleep(0.03)
    else:
        print("timeout")
