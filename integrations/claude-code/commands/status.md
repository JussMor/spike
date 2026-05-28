---
description: Show live vcs-overlay session status (agents, file counts, collisions)
allowed-tools: Bash(npx vcs-overlay *)
---

Live overlay state:

!`npx vcs-overlay status 2>&1 || echo "vcs-overlay not available — run from a project that depends on it"`

Summarize for the user: which sessions are active, how many files each has changed, and flag any collisions (the same file edited differently by two sessions). If they ask to act on a session, the full workflow is in the `vcs-overlay` skill.
