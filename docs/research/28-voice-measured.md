# 28. Voice, measured on this computer

Written 2026-11-22, directly. Built the same day, as proposed (DECISIONS V7, V8). Research note 25 named what voice mode can be built from and left three things to
measure first: the delay from the end of the person's sentence to the first sound of the answer, Smart Turn on
sentences stopped in the middle, and echo cancelling in the desktop webview. This note is the first two. The
third needs the desktop shell (plan item 10) and is not here.

How sure each line is: **Tried** (on the owner's computer, an i7-12700K with 20 threads and an RTX 3060 12 GB,
with `core/dev/voice-trial.py`, 22 November 2026; the voice parts on 4 processor threads, the language model on
the card), **Read** (from a maker's own page or repository, on 22 November 2026). Small numbers, mostly on
synthetic speech: direction, not a benchmark.

## What changed since note 25 (Read)

- **sherpa-onnx 2.0 removed espeak-ng and piper-phonemize** (its changelog, "Remove espeak-ng and
  piper-phonemize from sherpa-onnx", #4055). Since then Kokoro in it reads text only through a lexicon, and a
  word not in the lexicon is skipped, not spoken. **Tried:** with Kokoro v1.0's English lexicon (178,401
  entries), "minutes", "what's", "kilometres" and "Dana" were not spoken at all; v0.19 without a lexicon
  speaks nothing ("No lexicon available"). A voice that drops words is not usable, so Kokoro is out unless
  Demido gives it what it cannot say (below).
- **sherpa-onnx has a Rust crate of its own** now, `sherpa-onnx` 2.0.2 on crates.io, Apache-2.0, from the
  same repository: the core can use the engine measured here directly, without `sherpa-rs`.
- **Pocket TTS** (Kyutai, January 2026) runs in sherpa-onnx 2.x and needs no phonemizer: it reads the text as
  it is written. Weights CC-BY-4.0 (Kyutai's page and the ONNX conversion's); the conversion's code
  Apache-2.0. Kyutai's page adds conditions of use: no "voice impersonation or cloning without explicit and
  lawful consent", no deception, no unlawful or harmful content. It takes its voice from a recording, so
  Demido offers only voices whose speakers agreed to it.
- **Voices for it (Read):** Kyutai's voice set (`kyutai/tts-voices`) holds 228 voices donated by volunteers
  for this use, CC0; VCTK's, CC-BY-4.0; and Expresso's, CC-BY-NC-4.0, non-commercial, which Demido leaves out.

## Licences of what was measured (Read)

| Part | What | Licence |
|---|---|---|
| Engine | sherpa-onnx 2.0.1 (Python here; Rust crate 2.0.2) | Apache-2.0 |
| Engine | onnxruntime 1.31.1 | MIT |
| Speech detector | Silero VAD | MIT |
| Speech to text | NVIDIA Parakeet TDT 0.6B v3, int8 | CC-BY-4.0 (credit owed) |
| Speech to text | Whisper large-v3-turbo, int8 | MIT |
| Speech out | Kokoro-82M v1.0 and v0.19, int8 | Apache-2.0 |
| Speech out | Pocket TTS, int8 | CC-BY-4.0, with Kyutai's conditions of use |
| Sentence ended? | Smart Turn v3.2 (CPU build) | BSD-2-Clause |
| Its test set | pipecat-ai/smart-turn-data-v3.1-test | CC-BY-4.0 |
| For the trial only | transformers (Smart Turn's own feature extractor) | Apache-2.0 |

All compatible with GPL-3.0-or-later. The models are downloaded on demand, as models are (H7); none is in
Demido's repository.

## The trial (Tried)

Ten everyday requests ("What's the weather like in Rome tomorrow?", "Set a timer for twelve minutes."), and
eight that stop short ("What's the weather like in", "Remind me to call my mother when"), spoken by Pocket TTS in
the two voices its package ships. Real speech: the recordings the speech-to-text packages ship (three English
ones with transcripts). For Smart Turn also one shard of nine of its makers' own test set: 3,497 real and
synthetic recordings in 23 languages, 873 of them English, each labelled finished or not. Two full runs; the
numbers moved by 20 to 30% between them (the second ran slower throughout), so both are given where it matters.

## What came of it

**Speech to text**, on the processor:

| | Parakeet TDT 0.6B v3 | Whisper large-v3-turbo |
|---|---|---|
| A spoken request of 2.3 to 2.4 s | median **133 to 184 ms** | median 1.27 to 1.48 s |
| Word errors, three real English recordings | 2.5% | 2.1% |
| On the synthetic requests | right but for spelling (summarize, kilometers) in most; twice a word too many at the start; **twice nothing at all**, both in one voice | right but for numbers written as digits |

**Speech out**, on the processor:

| | Kokoro v1.0 (int8) | Pocket TTS (int8) |
|---|---|---|
| First sound of a sentence | median 1.48 to 1.63 s (it makes the sentence whole first) | median **0.68 to 0.79 s** (it streams, in pieces of 1.2 s of sound) |
| Work per second of speech | 0.63 to 0.69 s | 0.36 to 0.51 s |
| Every word spoken | no: words not in its lexicon are skipped | yes (heard back by Parakeet) |

More threads do not bring Pocket TTS's first sound sooner: 2 threads 0.65 to 0.95 s, 4 threads 0.57 to 0.89 s,
8 threads 0.86 to 1.19 s. Reading a voice the first time takes 0.24 to 0.72 s, once per voice. One sentence in
the second run took 5.5 s to its first sound; not looked into.

**Has the sentence ended?** Smart Turn v3.2, 38 to 47 ms a judgement on the processor:

| Speech | Judged finished |
|---|---|
| Its own test set, English, real and synthetic (873) | **95.6% right** (835); unfinished taken for finished 26 (3.0%), finished taken for unfinished 12 |
| Its own test set, all 23 languages (3,497) | 93.1% right (its makers report 94.3% over the whole set) |
| The real recordings (10, all finished) | 10 of 10 |
| Pocket TTS's finished requests (20) | 16 |
| Pocket TTS's requests that stop short (16) | 6 to 8, wrongly |
| Pocket TTS's finished requests cut in the middle (20) | 7 to 11, wrongly |

1. **Smart Turn is asked right here**: on its own test set this trial gets what its makers report. On real English
   speech it takes about one sentence in thirty-three that is not finished for finished.
2. **Pocket TTS's speech misleads it**: a third or more of its unfinished requests sound finished to Smart Turn. So
   Pocket TTS's speech says nothing about how Smart Turn does on a person's; the person's own voice will
   (below).
3. **Qwen3 4B's first sentence** of an answer, on the card, came after a median 123 ms (slowest 280 ms).

## The delay to the first sound, from these numbers

| Step | Time |
|---|---|
| Silence before Smart Turn is asked | 200 ms (a setting) |
| Smart Turn | 40 to 50 ms |
| Speech to text, Parakeet | 130 to 180 ms, or nothing added if it runs during the silence |
| The model's first sentence, Qwen3 4B | about 120 ms |
| Speech out, Pocket TTS, to its first sound | 680 to 790 ms |
| **In all** | **about 1.0 to 1.35 s** |

Half a second is what makes a conversation feel like one, note 25 said, and two seconds is not. This is between.
Most of it is the first piece of speech: sherpa-onnx gives Pocket TTS's sound in pieces of 1.2 s, so the first
sound waits until 1.2 s of speech is made. Kyutai's own streaming of the same model works in much smaller
steps (Known); driving its ONNX parts directly, through `ort`, could give the first sound after a fraction of
that. With Kokoro instead the delay would be nearer two seconds, and it drops words.

## Proposed: the first build (for the owner)

1. **In the core, on the processor:** Silero VAD for speech; Smart Turn v3.2 (through `ort`, with Whisper's
   features computed in Rust and checked against the trial's) after 200 ms of silence; when it judges the
   sentence unfinished, Demido waits, with no time limit, as V2 asks, and "Answer now" ends the wait.
2. **Speech to text: Parakeet TDT 0.6B v3**, through the `sherpa-onnx` crate, started as soon as the silence
   starts. When it hears nothing in speech the detector heard, Whisper turbo is asked instead (1.3 to 1.5 s
   more), so that nothing said is lost. Parakeet covers 25 European languages (Known).
3. **Speech out: Pocket TTS**, streamed, a sentence at a time, stopped at once when the person speaks (V3).
   Its voices only from Kyutai's donated voices (CC0); never a voice made from a recording of someone else.
   Pocket TTS speaks English (Known); other languages need another engine.
4. **Measured again on the person's own voice**, once the window can record: Smart Turn and Parakeet on their
   sentences, finished and not. Synthetic speech has said what it can.

Not measured: echo cancelling in the desktop webview (it needs the shell); the voice parts and the model
running at the same time; any of it on a computer without a graphics card.

## For the owner

- Whether the first build speaks English only (Pocket TTS does), and hears in 25 languages (Parakeet does).
- Which voices: Kyutai's donated voices are 228; a few chosen to start, or all of them offered.
- Whether a person may give a recording of their own voice for Demido to speak with. Kyutai's terms allow it
  with consent; a recording of anyone else, never.
- Speech to text never goes to a provider by default, as nothing does (note 25); whether it may at all.
