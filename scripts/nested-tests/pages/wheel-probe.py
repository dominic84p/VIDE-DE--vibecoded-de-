# A stand-in for vim/htop: asks the terminal for SGR mouse reports on the alternate screen and
# shows every byte it receives in the window title (control bytes as ^C, ^X...), so a nested
# test can read what it was sent.
# With --plain it skips the mouse modes and prints a line to select instead.
import os, sys, tty

plain = "--plain" in sys.argv
tty.setraw(0)
out = sys.stdout
if plain:
    out.write("\x1b[2J\x1b[Hmarker\r\n\x1b]0;probe ready\x07")
else:
    out.write("\x1b[?1049h\x1b[?1000h\x1b[?1006h\x1b]0;probe ready\x07")
out.flush()
seen = b""
while True:
    seen = (seen + os.read(0, 64))[-120:]
    shown = "".join("ESC" if ch == "\x1b" else "^" + chr(64 + ord(ch)) if ord(ch) < 32 else ch for ch in seen.decode(errors="replace"))
    out.write("\x1b]0;probe got " + shown + "\x07")
    out.flush()
