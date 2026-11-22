# 25. What voice mode can be built from, on this computer

Written 2026-11-22, directly. Plan item 9 starts with voice ("the libraries, the screen"), and DECISIONS V6
lists what the real thing needs, none of it chosen: a detector for speech, a small model that judges whether a
sentence is finished (V2), echo cancelling so Demido does not hear itself (V3), a speech engine that can stop
mid-word and say how far it got (V3), and speech to text. Nothing here is built.

How sure each line is: **Read** (licence and date read from the source on 22 November 2026: the Hugging Face
or GitHub page, or crates.io), **Known** (how the thing works, from its documentation as I know it, to be
confirmed when it is built), **Not measured** (needs a trial on this computer).

## Where the sound is

The window is a web page, in a browser or in the desktop shell. So the microphone is the page's
(`getUserMedia`), and the browser gives **echo cancelling, noise suppression and level control** with it, as
constraints of the capture (`echoCancellation`, `noiseSuppression`, `autoGainControl`, W3C Media Capture):
Chromium's is WebRTC's own, the same one video calls use (Known). That answers V3's echo without a library
of Demido's, for the browser. **Not measured**: whether the desktop shell's webview has it too (WebKitGTK on
Linux does WebRTC through GStreamer; that its capture honours `echoCancellation` must be tried), which bears on
the shell's choice (plan item 10).

So the shape is: the page sends 16 kHz sound to the core as it is captured, and plays what the core sends
back; everything that listens or speaks runs in the core, beside the models.

## Candidates

| Part | Candidate | Licence (Read) | Last change (Read) | Notes |
|---|---|---|---|---|
| Speech detector | Silero VAD (`snakers4/silero-vad`) | MIT | 2026-11-20 | ONNX, a few MB, runs on the processor in under a millisecond a frame (Known) |
| Sentence finished? (V2) | Smart Turn v3 (`pipecat-ai/smart-turn-v3`) | BSD-2-Clause | 2026-01-07 | ONNX, versions to 3.2 for processor and card; judges from the sound itself whether a turn is over, in several languages: what V2 asks for |
| Speech to text | Whisper, through whisper.cpp (`ggml-org/whisper.cpp`) | MIT (program); MIT (`openai/whisper-large-v3-turbo` weights) | 2026-11-20 | The same ggml family as the runtime; Rust binding `whisper-rs` 0.16.0, Unlicense |
| Speech to text | Parakeet TDT 0.6B v3 (`nvidia/parakeet-tdt-0.6b-v3`) | CC-BY-4.0 (weights: credit owed) | 2026-08-05 | Fast and accurate in European languages (Known); through sherpa-onnx |
| Text to speech | Kokoro-82M (`hexgrad/Kokoro-82M`) | Apache-2.0 | 2025-04-10 | Small (82M parameters), good voices; through sherpa-onnx or ONNX directly |
| Text to speech | Kitten TTS nano (`KittenML/kitten-tts-nano-0.2`) | Apache-2.0 | 2025-08-30 | Tiny, for the processor |
| Text to speech | Piper (`OHF-Voice/piper1-gpl`) | GPL-3.0 | 2026-11-21 | Compatible with Demido's licence; each voice has a licence of its own, to read one by one |
| All of them in one | sherpa-onnx (`k2-fsa/sherpa-onnx`) | Apache-2.0 | 2026-11-21 | Speech detection, Whisper, Parakeet and Kokoro behind one library (Known); Rust binding `sherpa-rs` 0.6.8, MIT |
| ONNX models from Rust | `ort` 2.0.0-rc.15 | MIT OR Apache-2.0 | | For Smart Turn, which sherpa-onnx does not run |
| Sound on the desktop | `cpal` 0.18.2 | Apache-2.0 | | Only if the core ever captures sound itself; the window does today |

Left out until read: `webrtc-audio-processing` 2.1.0 and `voice_activity_detector` 0.2.1 (crates.io says their
licence is "non-standard"); Moonshine (not found under the names tried).

## Stopping mid-word, and knowing how far it got (V3)

Speech is made a sentence at a time and sent as it is made, so the first sound comes after the first sentence,
not the whole answer. Each piece has a known length. When the person speaks over it, the page stops playing at
once and tells the core how many milliseconds of which piece were heard; the core keeps the sentences heard
whole, and of the one cut short the part in proportion to its time, and marks the rest "Stopped here" (V3).
**Known** for Kokoro: it can give the length of each sound it makes, which would place the cut on a word rather
than by proportion; to be confirmed.

## What runs beside the models

On the owner's card (12 GB), with a 4B model loaded: Silero and Smart Turn on the processor; Whisper large-v3-
turbo about 1.6 GB on the card, or Parakeet about 1.2 GB, or either on the processor at some seconds a sentence;
Kokoro about 330 MB, on either. **Not measured**: the time from the end of a sentence to the first sound back,
which is what makes a conversation feel like one (half a second is good, two seconds is not).

## A first build, proposed

1. sherpa-onnx in the core for the speech detector, speech to text (Whisper or Parakeet, the person's choice)
   and Kokoro; Smart Turn through `ort`. The models downloaded on demand from their makers, checked, kept in
   Demido's folder, as the runtime and the models are (H7).
2. The page captures with echo cancelling, sends sound over one connection, and plays what comes back.
3. Measure first, on this computer: the delay to the first sound, Smart Turn on sentences stopped in the
   middle, and echo cancelling in the desktop webview.

Not a decision: the owner's to say whether the first build is English only, which voices, and whether speech
to text may ever go to a provider (it never does by default, as nothing does).
