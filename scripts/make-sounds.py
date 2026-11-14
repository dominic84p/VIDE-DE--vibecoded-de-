#!/usr/bin/env python3
"""Synthesise VIDE-DE's notification sounds into assets/sounds/*.wav.

Usage: scripts/make-sounds.py [NAME...]   (no names: all of them; needs numpy for the studio set)

Original sounds in the spirit of Windoze 10's: short, soft, mallet-like. Nothing here is
sampled from anyone's sound files; every sample is computed below. Run it again to tweak.
"""
import math, os, struct, sys, wave

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
ONLY = set(sys.argv[1:])
for name, (length, voices) in SOUNDS.items():
    if ONLY and name not in ONLY:
        continue
    samples = render(length, voices)
    with wave.open(os.path.join(OUT, f"{name}.wav"), "wb") as w:
        w.setnchannels(1)
        w.setsampwidth(2)
        w.setframerate(RATE)
        w.writeframes(b"".join(struct.pack("<h", int(max(-1, min(1, s)) * 32767)) for s in samples))
    print(f"{name}.wav: {length}s")


# ---------------------------------------------------------------------------------------------
# The studio set: 48 kHz stereo, built with numpy. Modal synthesis (each partial its own pitch,
# level and decay), plucked strings (Karplus-Strong), soft mallet/breath transients, slight
# left/right detune for width, and a convolution reverb from a decaying noise impulse.

import numpy as np

SR = 48000
rng = np.random.default_rng(7)  # fixed seed: the same sounds every run


def t_axis(length):
    return np.arange(int(length * SR)) / SR


def adsr(t, attack=0.003, decay=0.3):
    return np.minimum(1.0, t / attack) * np.exp(-t / decay)


def release(x, ms=60):
    """Fade a voice's last few ms, so a note cut off at its length doesn't click."""
    n = min(len(x), int(ms / 1000 * SR))
    x[-n:] *= 0.5 + 0.5 * np.cos(np.linspace(0, np.pi, n))
    return x


def dc_block(x, cutoff=30.0):
    """High-pass below hearing: strips DC and sub-bass rumble from noise and plucks."""
    a = math.exp(-2 * math.pi * cutoff / SR)
    y = np.empty_like(x)
    prev_x = prev_y = 0.0
    for i, v in enumerate(x):
        prev_y = a * (prev_y + v - prev_x)
        prev_x = v
        y[i] = prev_y
    return y


def modal(freq, length, modes, attack=0.002, detune=0.0):
    """Struck object: modes are (ratio, level, decay seconds)."""
    t = t_axis(length)
    out = np.zeros_like(t)
    for ratio, level, decay in modes:
        f = freq * ratio * (1 + detune)
        out += level * np.sin(2 * np.pi * f * t + rng.uniform(0, 2 * np.pi)) * np.exp(-t / decay)
    return release(out * np.minimum(1.0, t / attack))


def lowpass(x, cutoff):
    """One-pole low-pass (run twice for 12 dB/oct)."""
    a = math.exp(-2 * math.pi * cutoff / SR)
    for _ in range(2):
        y = np.empty_like(x)
        acc = 0.0
        for i, v in enumerate(x):
            acc = (1 - a) * v + a * acc
            y[i] = acc
        x = y
    return x


def mallet(length, cutoff=3000, decay=0.006, level=0.25):
    """The felt-tip thump of a strike: a few ms of filtered noise."""
    t = t_axis(length)
    n = rng.standard_normal(len(t)) * np.exp(-t / decay)
    return level * lowpass(n, cutoff)


def pluck(freq, length, brightness=0.5, decay=0.996):
    """Karplus-Strong string: a burst of noise circulating in a tuned, damped delay line."""
    n = int(length * SR)
    period = SR / freq
    p = int(period)
    frac = period - p
    buf = lowpass(rng.uniform(-1, 1, p + 2), 1500 + 9000 * brightness)
    buf -= buf.mean()  # any DC would circulate in the string forever as a low rumble
    out = np.zeros(n)
    line = list(buf)
    idx = 0
    for i in range(n):
        a = line[idx % len(line)]
        b = line[(idx + 1) % len(line)]
        v = (1 - frac) * a + frac * b
        out[i] = v
        line[idx % len(line)] = decay * 0.5 * (a + b)
        idx += 1
    return release(out * np.minimum(1.0, np.arange(n) / (0.002 * SR)))


def place(track, sound, at, gain=1.0):
    start = int(at * SR)
    end = min(len(track), start + len(sound))
    track[start:end] += gain * sound[: end - start]


def reverb(stereo, size=1.2, wet=0.25, tone=6000):
    """Convolve with a decorrelated, darkening noise tail per channel."""
    t = t_axis(size)
    out = []
    for ch in range(2):
        ir = rng.standard_normal(len(t)) * np.exp(-t * 6.9 / size)  # -60 dB at `size`
        ir = lowpass(ir, tone)
        ir[: int(0.012 * SR)] *= np.linspace(0, 1, int(0.012 * SR))  # a little pre-delay
        ir /= np.sqrt(np.sum(ir ** 2))
        n = len(stereo[ch]) + len(ir) - 1
        size_fft = 1 << (n - 1).bit_length()
        tail = np.fft.irfft(np.fft.rfft(stereo[ch], size_fft) * np.fft.rfft(ir, size_fft), size_fft)[: len(stereo[ch])]
        out.append((1 - wet) * stereo[ch] + wet * tail)
    return out


def stereo_of(make, spread=0.0015):
    """Render a voice twice, a hair sharp on the left and flat on the right, for width."""
    return [make(-spread), make(spread)]


def write_wav(path, data):
    pcm = np.clip(data * 32767, -32768, 32767).astype("<i2")
    with wave.open(path, "wb") as w:
        w.setnchannels(2)
        w.setsampwidth(2)
        w.setframerate(SR)
        w.writeframes(pcm.tobytes())


def loudness(path):
    """Integrated loudness (LUFS) by ffmpeg's EBU R128 meter, or None without ffmpeg."""
    import re, subprocess
    try:
        out = subprocess.run(["ffmpeg", "-hide_banner", "-nostats", "-i", path, "-af", "ebur128", "-f", "null", "-"],
                             capture_output=True, text=True).stderr
    except FileNotFoundError:
        return None
    m = re.findall(r"I:\s+(-?[\d.]+) LUFS", out)
    return float(m[-1]) if m else None


TARGET_LUFS = -18.5  # where the classic set sits, so switching sounds doesn't change volume
CEILING = 10 ** (-1 / 20)  # -1 dBFS


def finish(name, left, right, length):
    n = int(length * SR)
    left, right = left[:n], right[:n]
    fade = np.minimum(1.0, (n - np.arange(n)) / (0.04 * SR))
    data = np.stack([left, right], axis=1) * fade[:, None]
    data /= max(1e-9, np.max(np.abs(data)))
    path = os.path.join(OUT, f"{name}.wav")
    # Peak-normalise, measure, then set the level by loudness; the peak ceiling wins if they disagree.
    data *= 0.5
    write_wav(path, data)
    lufs = loudness(path)
    if lufs is not None:
        data *= min(10 ** ((TARGET_LUFS - lufs) / 20), CEILING / np.max(np.abs(data)))
        write_wav(path, data)
    print(f"{name}.wav: {length}s stereo, {loudness(path) or float('nan'):.1f} LUFS, peak {20 * np.log10(np.max(np.abs(data))):.1f} dBFS")


def mix(length, parts, verb=None):
    """parts: (at seconds, pan -1..1, gain, stereo-renderer)."""
    n = int(length * SR)
    L, R = np.zeros(n), np.zeros(n)
    for at, pan, gain, voice in parts:
        l, r = voice
        place(L, l, at, gain * math.cos((pan + 1) * math.pi / 4) * math.sqrt(2))
        place(R, r, at, gain * math.sin((pan + 1) * math.pi / 4) * math.sqrt(2))
    if verb:
        L, R = reverb([L, R], **verb)
    return dc_block(L), dc_block(R)


MARIMBA_BAR = [(1, 1.0, 0.35), (3.99, 0.35, 0.08), (10.65, 0.08, 0.03)]
VIBES = [(1, 1.0, 1.1), (4.0, 0.3, 0.25), (10.0, 0.05, 0.08)]
BOWL = [(1, 1.0, 3.0), (1.006, 0.6, 2.8), (2.71, 0.45, 1.6), (2.725, 0.3, 1.5), (5.18, 0.15, 0.7)]
KALIMBA = [(1, 1.0, 0.6), (5.4, 0.25, 0.07), (11.1, 0.06, 0.03)]
CRYSTAL = [(1, 1.0, 0.9), (2.32, 0.35, 0.5), (4.25, 0.15, 0.25), (6.8, 0.06, 0.12)]


def struck(freq, length, modes, thump=0.15):
    return stereo_of(lambda d: modal(freq, length, modes, detune=d) + mallet(length, level=thump))


def studio():
    hz = lambda n: 440 * 2 ** ((n - 69) / 12)  # MIDI note to Hz

    # Aurora: a soft vibraphone arpeggio up an E major 9 chord, washed in reverb.
    notes = [64, 68, 71, 75, 78]
    L, R = mix(2.0, [(i * 0.075, -0.5 + i * 0.25, 1.0 - i * 0.08, struck(hz(n), 1.6, VIBES, 0.05)) for i, n in enumerate(notes)],
               verb=dict(size=1.6, wet=0.32, tone=5000))
    finish("aurora", L, R, 2.0)

    # Marimba: three warm rosewood bars, a rising fourth then a fifth.
    L, R = mix(1.0, [(0.0, -0.3, 1.0, struck(hz(72), 0.9, MARIMBA_BAR, 0.3)),
                     (0.09, 0.0, 0.9, struck(hz(77), 0.9, MARIMBA_BAR, 0.3)),
                     (0.18, 0.3, 1.0, struck(hz(84), 0.8, MARIMBA_BAR, 0.3))],
               verb=dict(size=0.8, wet=0.18, tone=7000))
    finish("marimba", L, R, 1.0)

    # Crystal: two glass harmonics that shimmer as their close partials beat.
    L, R = mix(1.6, [(0.0, -0.4, 1.0, struck(hz(88), 1.4, CRYSTAL, 0.04)),
                     (0.11, 0.4, 0.8, struck(hz(95), 1.3, CRYSTAL, 0.04))],
               verb=dict(size=1.8, wet=0.35, tone=9000))
    finish("crystal", L, R, 1.6)

    # Kalimba: a thumb-piano tine plucked twice, the second a sixth up.
    L, R = mix(1.1, [(0.0, -0.2, 1.0, struck(hz(74), 1.0, KALIMBA, 0.2)),
                     (0.12, 0.2, 0.85, struck(hz(83), 0.9, KALIMBA, 0.2))],
               verb=dict(size=0.9, wet=0.2, tone=6000))
    finish("kalimba", L, R, 1.1)

    # Harp: a quick plucked-string run up a pentatonic scale.
    run = [67, 69, 71, 74, 76, 79]
    L, R = mix(1.8, [(i * 0.045, -0.6 + i * 0.24, 0.9, stereo_of(lambda d, n=n: pluck(hz(n) * (1 + d), 1.4, 0.55, 0.9965)))
                     for i, n in enumerate(run)], verb=dict(size=1.4, wet=0.28, tone=6500))
    finish("harp", L, R, 1.8)

    # Zen: a singing bowl, its doubled partials beating slowly as it rings out.
    L, R = mix(3.2, [(0.0, 0.0, 1.0, struck(hz(69), 3.2, BOWL, 0.12))], verb=dict(size=2.4, wet=0.3, tone=4500))
    finish("zen", L, R, 3.2)

    # Droplet: one water drop, its pitch rising as the bubble closes.
    def drop(d):
        t = t_axis(0.35)
        f = 900 * (1 + d) * (1 + 1.6 * (1 - np.exp(-t / 0.03)))
        return release(np.sin(2 * np.pi * np.cumsum(f) / SR) * adsr(t, 0.001, 0.06), 30)
    L, R = mix(0.7, [(0.0, -0.15, 1.0, stereo_of(drop)), (0.16, 0.25, 0.45, stereo_of(lambda d: drop(d + 0.25)))],
               verb=dict(size=0.9, wet=0.3, tone=7000))
    finish("droplet", L, R, 0.7)

    # Orbit: two clean synth blips, a fifth apart, bouncing left to right.
    def blip(freq):
        def v(d):
            t = t_axis(0.25)
            mod = np.sin(2 * np.pi * freq * 2 * t) * 1.2 * np.exp(-t / 0.04)  # a touch of FM sparkle
            return release(np.sin(2 * np.pi * freq * (1 + d) * t + mod) * adsr(t, 0.002, 0.07), 30)
        return stereo_of(v)
    L, R = mix(0.8, [(0.0, -0.5, 1.0, blip(hz(79))), (0.085, 0.5, 0.9, blip(hz(86)))], verb=dict(size=0.7, wet=0.22, tone=8000))
    finish("orbit", L, R, 0.8)


def fart():
    """The important one. A buzzy pulse wave (the lips) whose pitch wanders and sags, flapping
    open and shut a few dozen times a second, with breath noise, all muffled."""
    length = 1.1
    t = t_axis(length)
    # Pitch: starts around 95 Hz, wobbles, sags at the end; a little random jitter keeps it organic.
    jitter = lowpass(rng.standard_normal(len(t)), 12) * 40
    f0 = 95 + 18 * np.sin(2 * np.pi * 3.1 * t) + jitter - 45 * np.clip((t - 0.65) / 0.4, 0, 1) ** 2
    phase = 2 * np.pi * np.cumsum(np.maximum(f0, 35)) / SR
    pulse = np.tanh(6 * np.sin(phase)) + 0.4 * np.sin(2 * phase)  # squared-off, reedy
    # Lip flutter: amplitude chopping at ~27 Hz, a bit uneven.
    flutter = 0.55 + 0.45 * np.sin(2 * np.pi * (27 * t + 0.6 * np.sin(2 * np.pi * 1.7 * t))) ** 2
    breath = lowpass(rng.standard_normal(len(t)), 900) * 0.35
    # Envelope: a quick start, a sustained rasp, a sputtering tail.
    env = np.minimum(1.0, t / 0.02) * np.clip(1.15 - t / length, 0, 1) ** 0.6
    # Rounded off, so the sputters are wet little gaps rather than digital clicks.
    sputter = lowpass(1 - 0.8 * (t > 0.82) * (np.sin(2 * np.pi * 11 * t) < -0.2), 45)
    voice = lowpass((pulse * flutter + breath) * env * sputter, 700)
    voice = release(voice, 80)
    L, R = mix(1.3, [(0.0, 0.0, 1.0, [voice, voice * 0.97])], verb=dict(size=0.35, wet=0.12, tone=2500))
    finish("fart", L, R, 1.3)


if not ONLY or "fart" in ONLY:
    fart()

if not ONLY or ONLY & {"aurora", "marimba", "crystal", "kalimba", "harp", "zen", "droplet", "orbit"}:
    studio()
