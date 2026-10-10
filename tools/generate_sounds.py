"""Generate three original, quiet pairs of Yappers interaction sounds.

All three pairs are embedded in the app and can be previewed in Settings.
No recorded or third-party audio is used.
"""

import math
import random
import wave
from pathlib import Path

RATE = 44_100
ROOT = Path(__file__).resolve().parents[1]
APP_SOUNDS = ROOT / "src-tauri" / "sounds"
APP_SOUNDS.mkdir(parents=True, exist_ok=True)


def write(path: Path, samples: list[float]) -> None:
    frames = bytearray()
    for sample in samples:
        clipped = max(-1.0, min(1.0, sample))
        frames.extend(int(clipped * 32767).to_bytes(2, "little", signed=True))
    with wave.open(str(path), "wb") as wav:
        wav.setnchannels(1)
        wav.setsampwidth(2)
        wav.setframerate(RATE)
        wav.writeframes(frames)


def airy(opening: bool) -> list[float]:
    """A rounded breath and a faint upward or downward pitch bend."""
    duration = 0.20 if opening else 0.17
    rng = random.Random(20 if opening else 21)
    fast = slow = phase = 0.0
    result = []
    for index in range(round(duration * RATE)):
        x = index / (duration * RATE)
        swell = math.sin(math.pi * x) ** 1.7
        white = rng.uniform(-1.0, 1.0)
        brightness = (0.16 + 0.17 * x) if opening else (0.33 - 0.17 * x)
        fast += brightness * (white - fast)
        slow += 0.025 * (white - slow)
        breath = (fast - slow) * swell * 0.115
        frequency = (390 + 180 * x) if opening else (560 - 200 * x)
        phase += 2 * math.pi * frequency / RATE
        rounded_tone = math.sin(phase) * (swell ** 1.5) * 0.016
        result.append(breath + rounded_tone)
    return result


def glass(opening: bool) -> list[float]:
    """A soft, diffuse glass shimmer with no hard attack."""
    duration = 0.25 if opening else 0.22
    base = 670 if opening else 520
    result = []
    for index in range(round(duration * RATE)):
        t = index / RATE
        attack = min(1.0, t / 0.018)
        decay = math.exp(-10 * t)
        shimmer = (
            math.sin(2 * math.pi * base * t)
            + 0.42 * math.sin(2 * math.pi * base * 2.37 * t)
            + 0.18 * math.sin(2 * math.pi * base * 3.91 * t)
        )
        result.append(0.061 * attack * decay * shimmer)
    return result


def whoosh(opening: bool) -> list[float]:
    """An unpitched, soft sweep that opens or closes in brightness."""
    duration = 0.19 if opening else 0.16
    rng = random.Random(30 if opening else 31)
    fast = slow = 0.0
    result = []
    for index in range(round(duration * RATE)):
        x = index / (duration * RATE)
        envelope = math.sin(math.pi * x) ** 1.4
        white = rng.uniform(-1.0, 1.0)
        cutoff = (0.09 + 0.36 * x) if opening else (0.45 - 0.36 * x)
        fast += cutoff * (white - fast)
        slow += 0.018 * (white - slow)
        result.append(0.15 * envelope * (fast - slow))
    return result


for name, generator in (("airy", airy), ("glass", glass), ("whoosh", whoosh)):
    pair = []
    for opening, label in ((True, "start"), (False, "stop")):
        samples = generator(opening)
        write(APP_SOUNDS / f"{name}-{label}.wav", samples)
        if not opening:
            pair.extend([0.0] * round(RATE * 0.22))
        pair.extend(samples)
