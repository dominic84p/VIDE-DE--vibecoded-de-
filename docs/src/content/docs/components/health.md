---
title: 'Health & Diagnostics (health.rs)'
description: 'Client ping watchdog, main-loop stall detector, TTY page-flip watchdog, and disconnect logging'
---

`src/health.rs` exists so a frozen app and a frozen compositor don't look identical in `~/vibe.log`.

## Client Ping Watchdog

Every **5 seconds** (`PING_EVERY`), `State::ping_tick` sends an `xdg_wm_base` ping to each enrolled Wayland client:

- **Grace period**: **3 seconds** (`GRACE`). If a client hasn't replied after 3 s:
  - It is logged by client name and PID in `~/vibe.log` (and again at **15 s**, **60 s**, then every minute).
  - Its window titlebars display **`(Not responding)`**, matching Windoze behavior.
- **Recovery**: When a previously stuck client answers a ping, recovery is logged and the `(Not responding)` suffix is removed.

## Watchdog Thread

A background watchdog thread monitors compositor responsiveness without touching main-loop state:

- **Main loop stall**: Logs a warning if the event loop makes no progress for **3 seconds**, along with the exact phase it is stalled in (`dispatching`, `rendering`, or `flushing`).
- **TTY page-flip stall**: Logs when a DRM/KMS page flip in `tty.rs` hasn't completed after **1.5 seconds**.

## Disconnect & Protocol Error Logging

When a Wayland client disconnects, `state.rs` logs:

- The client's name and PID (resolved from the socket).
- The full Wayland protocol error if the disconnect was caused by a protocol violation.

## Testing the Watchdog

`scripts/nested-tests/health.test` verifies both detectors using `scripts/nested-test.sh`:

1. **`stop` / `cont`**: Sends `SIGSTOP` to a running client, asserts `(Not responding)` / ping timeout appears in the log via `expect-log`, then sends `SIGCONT` and verifies recovery.
2. **`stall`**: Sends the `VIBE_TEST_INPUT=1`-only `stall` command over the control socket to wedge the main loop for 4 s and verifies the watchdog logs `main loop stuck`.
