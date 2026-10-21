---
description: Clean up the changed code (reuse, simplification, efficiency), then apply it
argument-hint: "[path]"
---
Look at the code that changed (`git diff HEAD` and untracked files${1:+, limited to $1}) and make it simpler without changing what it does.

Look for: duplicated logic that an existing helper already covers, abstractions with one caller, dead code and unused parameters, needless copies or repeated work in loops, error handling that can't trigger, comments that restate the code. Match the surrounding code's style.

Apply the changes, keep each one small, and run the project's typecheck or tests afterwards if it has them. End with a short list of what you changed and why.
