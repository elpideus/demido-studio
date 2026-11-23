#!/usr/bin/env python3
"""How soon Demido's voice can start (DECISIONS V8, V10). sherpa-onnx makes the whole of a sentence before it hands
over any of its sound (`offline-tts-pocket-impl.h`, GenerateSingleSentence: every frame until the end, then the
decoder), so the first sound of an answer comes after its whole first sentence is made. Two ways to bring it
sooner are measured here, each with what it costs in words heard right:

  1. fewer steps of the flow that makes each frame (`num_steps`, sherpa's default 5);
  2. the first sentence said in two pieces, split at its first comma (the rest made while the first plays).

What was said is written down again by Parakeet; the words it gets wrong are counted against the text.

    ~/.cache/demido-voice-trial/venv/bin/python -I core/dev/voice-pocket-steps-trial.py

Uses Demido's own voice files (~/.cache/demido-voice-files, voice 1410) and the processor, for a few minutes.
Downloads nothing."""
import os, re, sys, time, wave

import numpy as np
import sherpa_onnx

V = os.path.expanduser(os.environ.get("TRIAL_VOICE_DIR", "~/.cache/demido-voice-files"))
THREADS = int(os.environ.get("TRIAL_THREADS", "4"))
SENTENCES = [
    "Signs of ADHD in a child include difficulty focusing, hyperactivity, impulsivity, and trouble following instructions.",
    "It will be sunny in Rome tomorrow, with a high of nineteen degrees and a light wind from the sea.",
    "What is the keyboard model, and which characters come out wrong when you type?",
    "The train leaves at half past seven, so you should be at the station by seven fifteen.",
    "I found three flights to Lisbon next Friday, and the cheapest one costs one hundred and twelve euros.",
    "Before you change the battery, turn the device off and wait for the light to go out.",
    "Yes, I can do that.",
    "The meeting moved to Thursday afternoon, because two of the people could not come on Wednesday.",
]


def read_wav(path):
    with wave.open(path) as w:
        data = np.frombuffer(w.readframes(w.getnframes()), dtype=np.int16).astype(np.float32) / 32768
        if w.getnchannels() > 1:
            data = data.reshape(-1, w.getnchannels()).mean(1)
        return data, w.getframerate()


def words(text):
    return re.sub(r"[^a-z0-9' ]", " ", text.lower().replace("-", " ")).split()


def errors(said, heard):
    a, b = words(said), words(heard)
    d = list(range(len(b) + 1))
    for i, x in enumerate(a, 1):
        prev, d[0] = d[0], i
        for j, y in enumerate(b, 1):
            prev, d[j] = d[j], min(d[j] + 1, d[j - 1] + 1, prev + (x != y))
    return d[len(b)], len(a)


p = os.path.join(V, "pocket")
tts = sherpa_onnx.OfflineTts(sherpa_onnx.OfflineTtsConfig(model=sherpa_onnx.OfflineTtsModelConfig(
    pocket=sherpa_onnx.OfflineTtsPocketModelConfig(lm_flow=f"{p}/lm_flow.int8.onnx", lm_main=f"{p}/lm_main.int8.onnx", encoder=f"{p}/encoder.onnx", decoder=f"{p}/decoder.int8.onnx",
                                                   text_conditioner=f"{p}/text_conditioner.onnx", vocab_json=f"{p}/vocab.json", token_scores_json=f"{p}/token_scores.json"),
    num_threads=THREADS)))
q = os.path.join(V, "parakeet")
ear = sherpa_onnx.OfflineRecognizer.from_transducer(encoder=f"{q}/encoder.int8.onnx", decoder=f"{q}/decoder.int8.onnx", joiner=f"{q}/joiner.int8.onnx", tokens=f"{q}/tokens.txt",
                                                    num_threads=THREADS, model_type="nemo_transducer", decoding_method="greedy_search")
voice = read_wav(os.path.join(V, "voices", "1410.wav"))


def say(text, steps):
    g = sherpa_onnx.GenerationConfig()
    g.reference_audio, g.reference_sample_rate = voice[0].tolist(), voice[1]
    g.num_steps = steps
    first = []
    started = time.perf_counter()

    def given(samples, progress):
        if not first:
            first.append(time.perf_counter() - started)
        return 1

    audio = tts.generate(text, g, given)
    return np.asarray(audio.samples, dtype=np.float32), first[0] if first else time.perf_counter() - started


def heard(samples):
    stream = ear.create_stream()
    stream.accept_waveform(24000, samples)
    ear.decode_stream(stream)
    return stream.result.text.strip()


say("Hello.", 5)  # the voice is read once
print(f"Pocket TTS int8 on {THREADS} threads, voice 1410; {len(SENTENCES)} sentences\n")
for steps in (5, 3, 2, 1) if os.environ.get("TRIAL_STEPS", "1") == "1" else ():
    firsts, wrong, total, work, length = [], 0, 0, 0.0, 0.0
    for text in SENTENCES:
        started = time.perf_counter()
        samples, first = say(text, steps)
        work += time.perf_counter() - started
        length += len(samples) / 24000
        firsts.append(first)
        e, n = errors(text, heard(samples))
        wrong, total = wrong + e, total + n
    print(f"num_steps {steps}: first sound median {np.median(firsts):.2f} s, slowest {max(firsts):.2f} s; {work / length:.2f} s of work per second of speech; "
          f"words heard wrong {wrong}/{total} ({100 * wrong / total:.1f}%)", flush=True)

# The first sentence in two pieces: up to its first comma, and the rest; against the same sentence whole.
# Pocket draws each frame with some chance in it, so each is said ROUNDS times.
ROUNDS = int(os.environ.get("TRIAL_ROUNDS", "4"))
print()
whole, piece, wrong_whole, wrong_split, total = [], [], 0, 0, 0
for _ in range(ROUNDS):
    for text in SENTENCES:
        cut = text.find(", ")
        if cut < 0 or len(text[:cut].split()) < 3:
            continue
        w, first_whole = say(text, 5)
        a, first_piece = say(text[:cut + 1], 5)
        b, _ = say(text[cut + 2:], 5)
        whole.append(first_whole)
        piece.append(first_piece)
        e, n = errors(text, heard(w))
        f, _ = errors(text, heard(np.concatenate([a, b])))
        wrong_whole, wrong_split, total = wrong_whole + e, wrong_split + f, total + n
if whole:
    print(f"split at the first comma ({len(whole)} sayings): first sound median {np.median(piece):.2f} s, slowest {max(piece):.2f} s; "
      f"whole: median {np.median(whole):.2f} s, slowest {max(whole):.2f} s")
    print(f"words heard wrong: split {wrong_split}/{total} ({100 * wrong_split / total:.1f}%), whole {wrong_whole}/{total} ({100 * wrong_whole / total:.1f}%)", flush=True)
