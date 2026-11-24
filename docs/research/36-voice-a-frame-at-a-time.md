# 36. Demido's voice a frame at a time (IDEAS 146): what sherpa-onnx does, and what it would take

Written 2026-11-24, directly, for plan item 9 (the box "Demido's voice a frame at a time"). **Read only**: the
graphics card and the processor were held by a game all night, and the rule is no voice trials then, so
nothing here was run or measured. It says where the wait before the first sound comes from, and what a driver
of Demido's own needs, so that the build and its check can start once the computer is free.

How sure each line is: **Read** (sherpa-onnx 2.0.1's own source, `sherpa-onnx/csrc/offline-tts-pocket-impl.h`
and `offline-tts-pocket-model.cc`, Apache-2.0, fetched from its tag on GitHub; the model files Demido already
downloads, `~/.cache/demido-voice-files/pocket/`).

## Where the wait comes from (Read)

`GenerateSingleSentence` makes a sentence in two loops, one after the other:

1. **Every latent frame first.** The voice's embedding and then the text's (`text_conditioner.onnx` over the
   SentencePiece tokens) are fed to the main model (`lm_main.int8.onnx`) with an empty sequence, to set its
   state. Then, frame by frame (at most `max_frames`, 500): `lm_main` gives a conditioning vector and an
   end-of-speech logit; the flow model (`lm_flow.int8.onnx`) is run `num_steps` times from Gaussian noise
   (standard deviation √temperature, 0.7 by default), an Euler step each (`x += dt · v(s, t, x)`), which gives
   the frame's 32-value latent; that latent is the next step's input. Speech ends three frames after the
   logit first passes −4.
2. **Then the sound.** The decoder (`decoder.int8.onnx`, Mimi) takes the latents fifteen frames at a time
   (`chunk_size`), with a state of its own carried from chunk to chunk, and gives the audio; the callback is
   called per chunk.

So no sound comes before the whole sentence's latents exist, although the decoder is stateful and could take
the frames as they come. This is the likely reason for the 1.2 to 2.1 s before the first sound that DECISIONS
V10 measured; how much of it the latents take was not measured.

## What a driver of Demido's own needs

- The five ONNX parts through `ort`, as Smart Turn already runs: `text_conditioner`, `lm_main`, `lm_flow`,
  `decoder`, and `encoder` only for a voice made from a recording (Demido's voices can be embedded once and
  kept).
- Their states as sherpa-onnx makes them: zeros shaped from each session's own input description (every input
  of `lm_main` after the first two, every input of `decoder` after the first), carried from run to run.
- The tokenizer: SentencePiece (sherpa-onnx's `SentencePieceTokenizer`) over `vocab.json` and `token_scores.json` (by their names, its pieces and
  their scores; the files were not read); sherpa-onnx's text normalizers are optional.
- The loop of (1) as it is, and the decoder run **inside** it: on the first two or three frames, then on every
  few, so the first sound comes after a few frames' work rather than the sentence's. Stopping when the person
  speaks then stops between frames.
- Checked against sherpa-onnx's own sound: the same text, voice and seed through both, and the samples
  compared. The noise differs between the two unless the same noise is given to both (sherpa-onnx's
  `NormalDataGenerator` was not read), so the check feeds both the same noise.

About 300 lines of C++ to carry over, as IDEAS 146 guessed. **Not built, not tried.**
