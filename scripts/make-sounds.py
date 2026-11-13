#!/usr/bin/env python3
"""Synthesise VIDE-DE's notification sounds into assets/sounds/*.wav.

Original sounds in the spirit of Windoze 10's: short, soft, mallet-like. Nothing here is
sampled from anyone's sound files; every sample is computed below. Run it again to tweak.
"""
import math, os, struct, wave

RATE = 44100
OUT = os.path.join(os.path.dirname(__file__), "..", "assets", "sounds")


def note(freq, start, length, partials, decay, attack=0.004):
    """A struck tone: (ratio, level) partials, each dying away exponentially."""
    def f(t):
        t -= start
        if t < 0 or t > length:
            return 0.0
        env = min(1.0, t / attack) * math.exp(-t / decay)
        return env * sum(level * math.sin(2 * math.pi * freq * ratio * t) * math.exp(-t * (ratio - 1) * 3) for ratio, level in partials)
    return f


MARIMBA = [(1, 1.0), (3.9, 0.25), (9.2, 0.06)]
BELL = [(1, 1.0), (2.76, 0.45), (5.4, 0.2), (8.93, 0.08)]
GLASS = [(1, 1.0), (2.32, 0.3), (4.25, 0.12)]

SOUNDS = {
    # Default: two quick rising mallet notes.
    "ding": (0.9, [note(1318.5, 0.0, 0.8, MARIMBA, 0.18), note(1760.0, 0.09, 0.8, MARIMBA, 0.26)]),
    # Three rising bells.
    "chime": (1.3, [note(1046.5, 0.0, 1.2, BELL, 0.35), note(1318.5, 0.12, 1.2, BELL, 0.35), note(1568.0, 0.24, 1.0, BELL, 0.45)]),
    # A short soft pop.
    "pop": (0.25, [lambda t: math.sin(2 * math.pi * (900 * t - 1800 * t * t)) * math.exp(-t / 0.035) if t < 0.2 else 0.0]),
    # One bell, ringing out.
    "bell": (1.6, [note(880.0, 0.0, 1.6, BELL, 0.55)]),
    # Two light glassy taps.
    "glass": (0.7, [note(2093.0, 0.0, 0.6, GLASS, 0.12), note(2637.0, 0.07, 0.6, GLASS, 0.15)]),
}


def render(length, voices):
    n = int(length * RATE)
    raw = [sum(v(i / RATE) for v in voices) for i in range(n)]
    peak = max(1e-9, max(abs(x) for x in raw))
    # Fade the last 30 ms so nothing clicks, and leave headroom (-6 dBFS).
    fade = int(0.03 * RATE)
    return [x / peak * 0.5 * (min(1.0, (n - i) / fade)) for i, x in enumerate(raw)]


os.makedirs(OUT, exist_ok=True)
for name, (length, voices) in SOUNDS.items():
    samples = render(length, voices)
    with wave.open(os.path.join(OUT, f"{name}.wav"), "wb") as w:
        w.setnchannels(1)
        w.setsampwidth(2)
        w.setframerate(RATE)
        w.writeframes(b"".join(struct.pack("<h", int(max(-1, min(1, s)) * 32767)) for s in samples))
    print(f"{name}.wav: {length}s")
