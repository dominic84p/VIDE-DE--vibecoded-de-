# A stand-in for vim/htop: asks the terminal for SGR mouse reports on the alternate screen and
# shows every byte it receives in the window title, so a nested test can read what a wheel sent.
import os, sys, tty

tty.setraw(0)
out = sys.stdout
out.write("\x1b[?1049h\x1b[?1000h\x1b[?1006h\x1b]0;wheel ready\x07")
out.flush()
seen = b""
while True:
    seen = (seen + os.read(0, 64))[-120:]
    out.write("\x1b]0;wheel got " + seen.decode(errors="replace").replace("\x1b", "ESC") + "\x07")
    out.flush()
