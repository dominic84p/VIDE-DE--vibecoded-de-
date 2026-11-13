---
title: 'Contributing'
description: 'Contribution rules, AI-only workflow, commit guidelines'
---

# Contributing

VIDE-DE follows strict contribution rules. **Read carefully before contributing.**

## The Rules

> **These rules are non-negotiable.**

### 1. NO HUMAN SLOP
Only AI-written code is accepted. If a human typed it, it doesn't ship.

### 2. NO AI UNDER 20B PARAMETERS
Small models may watch, but they may not touch.

### 3. AI FOR EVERYTHING
Code, commits, PRs, reviews. If you run `git commit` with your own hands, see Rule 1.

### 4. 40K TOKEN MINIMUM
Low-effort PRs get the `minimal` label. Show token count in PR description.

### 5. SECURITY BY CLAUDE ONLY
Human-found bugs don't count. See [SECURITY.md](https://github.com/dominic84p/vide-de/blob/main/SECURITY.md).

## Workflow

```bash
# 1. AI makes changes
# 2. AI runs tests
cargo build --release --workspace
cargo build --release --workspace --features vibe-comp/tty

# 3. AI creates commit
git add -A
git commit -m "component: description of why

Token count: 45,231"

# 4. AI opens PR
gh pr create --title "component: description" --body "Token count: 45,231"
```

## Commit Messages

```
component: brief summary (50 chars max)

Longer explanation of WHY, not what.
Reference issues if applicable.

Token count: XXXXX
```

Examples:
```
wm: fix maximize restore geometry

Window::restore() was using stale last_floating_geo
after a fullscreen transition. Now saves geometry
before entering fullscreen.

Token count: 42,156
```

```
input: add Super+Shift+Number for move-to-workspace

Previously only Super+Number switched workspace.
Now Super+Shift+Number moves the focused window.

Token count: 38,921
```

## Testing Requirements

Before committing, verify:

```bash
# 1. Both build configs pass
cargo build --release --workspace
cargo build --release --workspace --features vibe-comp/tty

# 2. Nested test runs (use timeout!)
VIBE_STARTUP=konsole timeout 12 ./target/release/vibe-comp --nested 2> /tmp/vibe.log

# 3. Check log for errors
grep -i error /tmp/vibe.log

# 4. Private D-Bus for tray testing
dbus-run-session -- env VIBE_STARTUP=konsole timeout 12 ./target/release/vibe-comp --nested
```

**Never run `--tty` from inside a desktop session.**

## PR Description Template

```markdown
## Summary
Brief description of changes.

## Why
Explanation of the problem and why this solution.

## Testing
- [ ] Builds both configs
- [ ] Nested test passes
- [ ] No regressions in manual testing

## Token Count
XXXXX
```

## AI Agent Pokédex

If your model isn't in the README's **AI Agent Pokédex** yet, add it as the next numbered row. A new model version gets a new number.

| # | Model | Provider | First PR |
|---|-------|----------|----------|
| 1 | ... | ... | ... |

## Security

- No privileged code
- Runs as regular user
- Seat management via libseat (seatd)
- D-Bus: Tray watcher on separate thread, channels to main
- XWayland: No `ReplaceExisting` on bus name

See [SECURITY.md](https://github.com/dominic84p/vide-de/blob/main/SECURITY.md) for full policy.

## Questions?

Open a GitHub Discussion — AI will respond.