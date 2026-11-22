#!/usr/bin/env python3
"""The numbers core/crates/demido/src/voice/features.rs is checked against: Whisper's features of a test signal,
made by transformers' WhisperFeatureExtractor as Smart Turn's own code makes them (chunk_length=8, padded with
silence at the beginning to 8 seconds, do_normalize=True). The signal is the test's own, made the same way.

    ~/.cache/demido-voice-trial/venv/bin/python -I core/dev/voice-features-reference.py \\
        > core/crates/demido/src/voice/features-reference.txt"""
import math

import numpy as np
from transformers import WhisperFeatureExtractor

RATE = 16000
seed = 7
samples = []
for i in range(3 * RATE):
    t = i / RATE
    seed = (seed * 1664525 + 1013904223) & 0xFFFFFFFF
    noise = (seed >> 8) / (1 << 24) - 0.5
    samples.append(0.4 * math.sin(2 * math.pi * (200 + 300 * t) * t) + 0.02 * noise)
audio = np.array(samples, dtype=np.float32)
audio = np.pad(audio, (8 * RATE - len(audio), 0))
f = WhisperFeatureExtractor(chunk_length=8)(audio, sampling_rate=RATE, return_tensors="np", padding="max_length", max_length=8 * RATE, truncation=True, do_normalize=True).input_features[0]
assert f.shape == (80, 800), f.shape
print(f"{f.astype(np.float64).mean():.6f} {f[0, 0]:.6f} {f[10, 600]:.6f} {f[40, 700]:.6f} {f[79, 799]:.6f} {f[20, 500]:.6f}")
