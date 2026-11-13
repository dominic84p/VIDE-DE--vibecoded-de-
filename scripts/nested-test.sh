#!/usr/bin/env bash
# Run vibe-comp nested, safely, and drive it with a small script: launch apps, click, type,
# take screenshots. Everything is isolated from the session you're in:
#
#   - its own XDG_RUNTIME_DIR and D-Bus (apps start portals on the bus they were given; with
#     your real runtime dir a second xdg-document-portal remounts /run/user/UID/doc and your
#     Flatpak apps stop launching, see AGENTS.md)
#   - its own XDG_CONFIG_HOME, so settings.conf in your home is never read or written
#   - its own process group, killed as a group at the end (never pkill by name)
#   - screenshots go to the output folder, not ~/Pictures or the clipboard
#
# Usage: scripts/nested-test.sh [options] TESTFILE
#   -o DIR          where screenshots and logs go (default: target/nested-test)
#   --config FILE   a settings.conf to start with (default: none, so defaults)
#   --startup CMD   a program for the compositor to start, like VIBE_STARTUP
#   --real-bus      let `run-host` programs use your real D-Bus (read-only things, like
#                   `flatpak permissions`); apps given `run` never do
#   --timeout SECS  give up after this long (default 120)
#
# TESTFILE has one command per line (# starts a comment):
#   wait SECS                    sleep
#   run CMD...                   start a program as a client of the nested compositor
#   run-host CMD...              the same, but with your real runtime dir for read-only access
#                                to audio and systemd (still drawn on the nested screen)
#   expect-window TEXT [SECS]    (quote multi-word TEXT) wait until a window's app id or title contains TEXT, else fail
#   move X Y | click X Y [left|right|middle] | down | up
#   scroll N                     N mouse-wheel notches at the pointer, positive down
#   key NAME[+NAME...]           e.g. key enter, key ctrl+l   (a-z 0-9 esc tab enter space arrows ...)
#   type TEXT                    lowercase letters, digits, space and . / - , (capitals use shift)
#   shot NAME                    save OUT/NAME.png
#   stop N | cont N              SIGSTOP / SIGCONT the Nth `run` program (to fake a hang)
#   expect-log TEXT [SECS]       wait for TEXT in the compositor's log, else fail
#   windows                      print the window list
#   ipc COMMAND...               send any control-socket command and print the reply
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUT="$ROOT/target/nested-test"
CONFIG=""
STARTUP=""
REALBUS=0
TIMEOUT=120
TEST=""
while [ $# -gt 0 ]; do
  case "$1" in
    -o) OUT="$2"; shift 2 ;;
    --config) CONFIG="$2"; shift 2 ;;
    --startup) STARTUP="$2"; shift 2 ;;
    --real-bus) REALBUS=1; shift ;;
    --timeout) TIMEOUT="$2"; shift 2 ;;
    -h | --help) sed -n '2,37p' "$0"; exit 0 ;;
    *) TEST="$1"; shift ;;
  esac
done
[ -n "$TEST" ] && [ -f "$TEST" ] || { echo "usage: $0 [options] TESTFILE (see --help)" >&2; exit 2; }
[ -x "$ROOT/target/release/vibe-comp" ] || { echo "build first: cargo build --release" >&2; exit 2; }
: "${XDG_RUNTIME_DIR:?not inside a desktop session}" "${WAYLAND_DISPLAY:?not inside a Wayland session}"

HOST_WL="$XDG_RUNTIME_DIR/$WAYLAND_DISPLAY"
[ "${WAYLAND_DISPLAY#/}" != "$WAYLAND_DISPLAY" ] && HOST_WL="$WAYLAND_DISPLAY"
mkdir -p "$OUT"
# Short on purpose: Unix socket paths top out at about 108 characters.
RT="$(mktemp -d /tmp/vide-nt.XXXXXX)"
CF="$RT/config"; mkdir -p "$CF/vide-de"
[ -n "$CONFIG" ] && cp "$CONFIG" "$CF/vide-de/settings.conf"
# Session startup launches the KDE wallet when nothing on the (private) bus provides a keyring, and
# that pops a password prompt over every test. Nothing here needs a keyring.
mkdir -p "$RT/bin"
printf '#!/bin/sh\nexit 0\n' > "$RT/bin/ksecretd"; chmod +x "$RT/bin/ksecretd"

doc_mounts() { mount | grep -c "$XDG_RUNTIME_DIR/doc" || true; }
BEFORE="$(doc_mounts)"

RUNNER="$RT/runner.py"
cat > "$RUNNER" <<'PY'
import os, socket, subprocess, sys, time

OUT = os.environ["NT_OUT"]; RT = os.environ["NT_RT"]; ROOT = os.environ["NT_ROOT"]
HOST_RT = os.environ["NT_HOST_RT"]; REALBUS = os.environ.get("NT_REALBUS") == "1"
failed = False
result = open(os.environ['NT_RESULT'], 'w', buffering=1)
def P(*a, end='\n'):
    result.write(' '.join(str(x) for x in a) + end)

env = dict(os.environ, VIBE_TEST_INPUT="1")
# Stubs (see nested-test.sh) come first, for the compositor and for every client.
os.environ["PATH"] = os.path.join(RT, "bin") + ":" + os.environ["PATH"]
env["PATH"] = os.environ["PATH"]
if os.environ.get("NT_STARTUP"):
    env["VIBE_STARTUP"] = os.environ["NT_STARTUP"]
log = open(os.path.join(OUT, "compositor.log"), "w")
comp = subprocess.Popen([os.path.join(ROOT, "target/release/vibe-comp"), "--nested"], env=env, stderr=log, stdout=log)

def find(prefix):
    with open(os.path.join(OUT, "compositor.log"), errors="replace") as f:
        for line in f:
            if prefix in line:
                return line.split(prefix, 1)[1].strip().split()[0]
    return None

sock = None
for _ in range(100):
    time.sleep(0.2)
    sock = find("control socket at ")
    if sock: break
if not sock:
    P("FAIL: the compositor never came up; see compositor.log"); sys.exit(1)
wl = sorted(f for f in os.listdir(RT) if f.startswith("wayland-") and not f.endswith(".lock"))[0]
P(f"compositor up: {wl}")

def ipc(cmd):
    s = socket.socket(socket.AF_UNIX); s.settimeout(3); s.connect(sock)
    s.sendall((cmd + "\n").encode())
    buf = b""
    try:
        while True:
            d = s.recv(65536)
            if not d: break
            buf += d
    except socket.timeout:
        pass
    s.close()
    return buf.decode(errors="replace")

def xdisplay():
    """The nested Xwayland's display, latest first (it changes if Xwayland is restarted)."""
    found = None
    with open(os.path.join(OUT, "compositor.log"), errors="replace") as f:
        for line in f:
            if "Xwayland ready on " in line:
                found = line.split("Xwayland ready on ", 1)[1].strip().split()[0]
    return found

def client_env(host):
    e = dict(os.environ)
    e.pop("VIBE_TEST_INPUT", None)
    # Never the human's X server: an X11 app in a test would open on their real screen.
    e.pop("DISPLAY", None)
    if xdisplay():
        e["DISPLAY"] = xdisplay()
    if host:
        e["XDG_RUNTIME_DIR"] = HOST_RT
        e["WAYLAND_DISPLAY"] = os.path.join(RT, wl)       # absolute path to the nested socket
        if REALBUS:
            e["DBUS_SESSION_BUS_ADDRESS"] = f"unix:path={HOST_RT}/bus"
    else:
        e["WAYLAND_DISPLAY"] = wl
    return e

kids = []
for n, raw in enumerate(open(os.environ["NT_TEST"]), 1):
    line = raw.split("#", 1)[0].strip()
    if not line: continue
    cmd, _, rest = line.partition(" ")
    rest = rest.strip()
    P(f"> {line}")
    if cmd == "wait":
        time.sleep(float(rest))
    elif cmd in ("run", "run-host"):
        kids.append(subprocess.Popen(rest, shell=True, env=client_env(cmd == "run-host"),
                                     stdout=open(os.path.join(OUT, f"client{len(kids)+1}.log"), "w"), stderr=subprocess.STDOUT))
    elif cmd == "expect-window":
        import shlex
        words = shlex.split(rest)           # quotes allowed: expect-window "select test" 40
        needle, secs = words[0], (words[1] if len(words) > 1 else "")
        deadline = time.time() + float(secs or 20)
        while time.time() < deadline and needle not in ipc("windows"):
            time.sleep(0.4)
        if needle not in ipc("windows"):
            P(f"FAIL: no window matching {needle!r}"); failed = True
    elif cmd == "move":
        ipc(f"pointer-move {rest}")
    elif cmd == "click":
        x, y, *b = rest.split()
        ipc(f"pointer-move {x} {y}"); time.sleep(0.15); ipc(f"pointer-click {b[0] if b else 'left'}")
    elif cmd in ("down", "up"):
        ipc(f"pointer-{cmd}")
    elif cmd == "scroll":
        ipc(f"pointer-scroll {rest}")
    elif cmd == "key":
        ipc(f"key {rest}")
    elif cmd == "type":
        ipc(f"type {rest}")
    elif cmd == "shot":
        path = os.path.join(OUT, f"{rest}.png")
        if os.path.exists(path): os.remove(path)
        ipc(f"screenshot {path}")
        for _ in range(50):
            if os.path.exists(path) and os.path.getsize(path) > 0: break
            time.sleep(0.1)
        else:
            P(f"FAIL: no screenshot {path}"); failed = True
    elif cmd in ("stop", "cont"):
        import signal
        idx = int(rest) - 1                      # the Nth `run` command, counting from 1
        kids[idx].send_signal(signal.SIGSTOP if cmd == "stop" else signal.SIGCONT)
    elif cmd == "expect-log":
        import shlex
        words = shlex.split(rest)
        needle, secs = words[0], (words[1] if len(words) > 1 else "")
        deadline = time.time() + float(secs or 10)
        def logged(): return needle in open(os.path.join(OUT, "compositor.log"), errors="replace").read()
        while time.time() < deadline and not logged():
            time.sleep(0.4)
        if not logged():
            P(f"FAIL: never logged {needle!r}"); failed = True
    elif cmd == "windows":
        P(ipc("windows"), end="")
    elif cmd == "ipc":
        P(ipc(rest), end="")
    else:
        P(f"FAIL: line {n}: unknown command {cmd!r}"); failed = True

for k in kids:
    if k.poll() is not None: P(f"note: a client exited early (code {k.returncode})")
comp.terminate()
sys.exit(1 if failed else 0)
PY

PGID_FILE="$RT/pgid"
setsid bash -c "echo \$\$ > '$PGID_FILE'; exec env XDG_RUNTIME_DIR='$RT' WAYLAND_DISPLAY='$HOST_WL' XDG_CONFIG_HOME='$CF' \
  NT_OUT='$OUT' NT_RT='$RT' NT_ROOT='$ROOT' NT_HOST_RT='$XDG_RUNTIME_DIR' NT_REALBUS='$REALBUS' NT_STARTUP='$STARTUP' NT_TEST='$TEST' NT_RESULT='$OUT/result.txt' \
  dbus-run-session -- python3 '$RUNNER' >'$OUT/bus.log' 2>&1" &
RUNPID=$!
STATUS=0
for _ in $(seq 1 "$TIMEOUT"); do
  kill -0 "$RUNPID" 2>/dev/null || break
  sleep 1
done
if kill -0 "$RUNPID" 2>/dev/null; then echo "FAIL: timed out after ${TIMEOUT}s"; STATUS=1; fi
wait "$RUNPID" 2>/dev/null || STATUS=$?

# Only ever our own process group.
PGID="$(cat "$PGID_FILE" 2>/dev/null || true)"
cat "$OUT/result.txt" 2>/dev/null || true
if [ -n "$PGID" ]; then
  kill -TERM -- "-$PGID" 2>/dev/null || true; sleep 1; kill -KILL -- "-$PGID" 2>/dev/null || true
fi
AFTER="$(doc_mounts)"
if [ "$BEFORE" != "$AFTER" ]; then
  echo "WARNING: the document portal mount changed ($BEFORE -> $AFTER). Repair: systemctl --user restart xdg-document-portal" >&2
  STATUS=1
fi
rm -rf "$RT"
echo "output: $OUT"
exit "$STATUS"
