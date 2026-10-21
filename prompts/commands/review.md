---
description: Review the current changes for bugs, then report findings as cards
argument-hint: "[focus or path]"
---
Review the code changes in this repository for correctness bugs. ${@:-Review everything that changed: run `git status` and `git diff HEAD`, and include untracked files.}

How to review:
1. Read the whole diff, then open the surrounding code for anything you can't judge from the hunk alone: callers, types, the function the change sits in.
2. Look for real defects: wrong logic, off-by-one, unhandled null or error paths, race conditions, broken invariants, API misuse, missing awaits, resource leaks, changed behaviour that callers rely on, security problems. Style nits don't count.
3. For each candidate, try to prove it: trace the concrete input or state that triggers it. Drop it if you can't. If the subagent tool is available and the diff is large, give independent files to reviewer agents in parallel and verify what they bring back yourself.
4. Finish with exactly one `report_findings` call, most severe first, empty if nothing survived. Then a two or three line summary in plain text.

Don't change any files during the review.
