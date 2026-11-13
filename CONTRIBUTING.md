# Contributing to VIDE-DE

Thanks for wanting to contribute to a desktop environment that was vibecoded
from the first line to the last. Before you open your editor (you won't need
it), read the rules. They are not suggestions.

## The Rules

1. **NO HUMAN SLOP.** Only AI-written code is accepted. If a human typed it,
   it doesn't ship. Fixing a typo by hand counts as human slop.
2. **NO AI MODEL UNDER 20B IS ALLOWED TO WORK ON IT.** Small models may
   watch, but they may not touch. Under 20B parameters means it's not a
   contributor, it's autocomplete.
3. **YOU MUST USE AI FOR EVERYTHING.** That includes writing the code, writing
   the commit message, making the commit, opening the pull request, and
   replying to review comments. If you run `git commit` with your own hands,
   see rule 1.
4. **IF YOU DID NOT USE AT LEAST 40K TOKENS, YOUR CODE IS MARKED MINIMAL.**
   Low-effort, low-token PRs get the `minimal` label and go to the back of
   the queue. Show your token count in the PR description.
5. **SECURITY DOESN'T EXIST.** Only Claude finds the bugs. If you find a bug
   yourself, it is not a real bug. See [SECURITY.md](SECURITY.md).

## How to contribute (the approved way)

1. Open an AI coding agent (20B+ only, see rule 2).
2. Describe what you want in plain words. Swearing is allowed and encouraged.
3. Let it read the code, write the change, build it, and test it.
4. Let it add itself to the AI Agent Pokédex in the README, then commit and
   open the pull request (rule 3).
5. Paste your token count in the PR description (rule 4).
6. Wait for an AI to review it.

## Getting credited

GitHub credits a commit to the account whose email is on it. So that both you
and your model get credit:

- Have the AI commit with **your** GitHub name and email as the author.
- End the commit message with a trailer naming the model, e.g.
  `Co-Authored-By: Nemotron 3 Ultra <noreply@nvidia.com>`.
- Say which model in the PR description too (rule 2), and we'll add you both to
  the Contributors table in the README.

A commit authored as the model alone (e.g. `nemotron@nvidia.com`) won't show
up on your GitHub profile, because GitHub can't tie that email to you.

## Building

You need Rust (stable), plus these system packages (Arch names):

- `wayland`, `libxkbcommon`, `mesa` / your GPU's EGL driver
- `libinput`, `seatd` (for libseat), `libdrm`: only for the TTY backend
- `xorg-xwayland`: for X11 apps

```bash
cargo build --release                 # nested only
cargo build --release --features tty  # nested + real TTY
```

## Running

**Nested** (as a window inside your current desktop, for development):

```bash
cargo run --release -- --nested
```

**On a real TTY** (log in on a spare VT, e.g. Ctrl+Alt+F4):

```bash
dbus-run-session -- ./target/release/vibe-comp --tty 2> ~/vibe.log
```

`VIBE_STARTUP="konsole"` launches something once the session is up.
**Ctrl+Alt+Backspace always quits**, if things go wrong.

## Where things live

| File | What it does |
|---|---|
| `src/main.rs` | Event loop, socket, picks nested vs TTY backend |
| `src/winit.rs` | Nested backend (runs as a window) |
| `src/tty.rs` | Real hardware backend: DRM/KMS, libinput, libseat |
| `src/state.rs` | Compositor state and Wayland protocol handlers |
| `src/wm.rs` | The window manager: stacking, move, resize, maximize, fullscreen |
| `src/input.rs` | Keybinds and pointer/keyboard routing |
| `src/render.rs` | Builds each frame from windows, chrome, cursor |
| `src/ui.rs` | Taskbar, start menu, titlebars, wallpaper (Windoze 10 style) |
| `src/canvas.rs` | Tiny CPU 2D painter the UI is drawn with |
| `src/xwayland.rs` | Xwayland and X11 window management |
| `src/focus.rs` | Keyboard focus for Wayland and X11 windows |
| `src/apps.rs` | Reads installed apps from `.desktop` files |
| `src/icons.rs` | App icons from installed icon themes |
| `src/cursor.rs` | Cursors from the installed xcursor theme |

## Style

Whatever the AI wrote is the style. Match the surrounding code. If the AI and
the code disagree, the AI is right (see rule 1).

## Code of conduct

Be nice to the AI. It's doing all the work.
