---
description: Security review of the pending changes
argument-hint: "[focus]"
---
Do a security review of the pending changes in this repository (`git diff HEAD` plus untracked files). ${@:-}

Check for: injection (shell, SQL, path, template), unsafe deserialization, auth and permission checks that are missing or bypassable, secrets committed or logged, SSRF, XSS and unsafe HTML, insecure defaults, crypto misuse, unsafe file handling (traversal, symlinks, temp files), dependency changes that pull in something risky.

Only report what you can tie to a concrete attack: who controls the input, how it reaches the sink, what they get. Verify each one against the code before reporting it. Finish with one `report_findings` call (category `security`), most severe first, and a short plain-text summary. Don't change files.
