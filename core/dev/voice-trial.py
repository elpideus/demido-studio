#!/usr/bin/env python3
"""What voice mode can be built from, measured on this computer (research note 28; plan item 9; note 25).

The delay that makes a spoken conversation feel like one is from the end of the person's sentence to the
first sound of the answer. It is the sum of: how long the speech detector waits on silence, the model that
judges whether the sentence is finished (Smart Turn), speech to text, the language model's first sentence,
and speech out for that sentence. Each is measured here, on the processor, with the models note 25 names:

  speech to text   NVIDIA Parakeet TDT 0.6B v3 (int8) and Whisper large-v3-turbo, through sherpa-onnx;
                   on real speech (the recordings each package ships) and on sentences spoken by Pocket TTS
  speech out       Kokoro-82M v1.0 (int8), which makes a sentence whole before it sounds, and Kyutai's
                   Pocket TTS (int8), which streams: the time to a sentence's first sound. sherpa-onnx 2.x has
                   no espeak-ng (removed in 2.0, its changelog #4055), so Kokoro speaks only the words in its
                   lexicon and skips the rest; Pocket TTS needs no phonemizer
  sentence ended?  Smart Turn v3.2 (CPU build), through onnxruntime, with the Whisper features its own code
                   uses: on finished sentences, sentences that stop short, and finished ones cut in the middle
  first sentence   (TRIAL_LLM=1) Qwen3 4B through the installed runtime, on the graphics card

    core/dev/voice-trial-setup.sh            once: a Python of its own and the models, cached
    ~/.cache/demido-voice-trial/venv/bin/python -I core/dev/voice-trial.py

TRIAL_THREADS=4 (default) sets the processor threads; TRIAL_SHOW=1 prints every transcript. Small numbers,
on one computer, mostly on synthetic speech: direction, not a benchmark."""
import glob, json, os, re, secrets, signal, subprocess, sys, tempfile, time, urllib.request, wave

import numpy as np
import onnxruntime as ort
import sherpa_onnx
from transformers import WhisperFeatureExtractor

CACHE = os.environ.get("DEMIDO_VOICE_CACHE", os.path.expanduser("~/.cache/demido-voice-trial"))
M = os.path.join(CACHE, "models")
THREADS = int(os.environ.get("TRIAL_THREADS", "4"))
SHOW = bool(os.environ.get("TRIAL_SHOW"))

COMPLETE = [
    "What's the weather like in Rome tomorrow?",
    "Please add milk and eggs to my shopping list.",
    "Can you summarise the last email from Dana?",
    "Set a timer for twelve minutes.",
    "How many kilometres is it from Milan to Turin?",
    "I'd like to book a table for four people on Friday evening.",
    "Read me the first paragraph of the report.",
    "Turn the volume down a little, it's too loud.",
    "Who wrote the novel One Hundred Years of Solitude?",
    "Remind me to call my mother when I get home.",
]
# Sentences a person stops in the middle of: thinking, or about to go on.
SHORT = [
    "What's the weather like in",
    "Please add milk and",
    "Can you summarise the last email from",
    "I'd like to book a table for four people on",
    "Remind me to call my mother when",
    "How many kilometres is it from Milan to",
    "Read me the first",
    "Who wrote the novel",
]
SPOKEN_RATE = [24000]  # the sample rate of the speech the trial made
TRANSCRIPTS = {}  # a real recording, and what is said in it, where its package says
VOICES = [3, 6, 16, 21]  # four of Kokoro's English voices: American women's and a man's, a British woman's


def say_time(seconds):
    return f"{seconds * 1000:.0f} ms"


def words(text):
    text = text.lower().replace("'", "")
    return re.findall(r"[a-z0-9]+", text)


def wer(said, heard):
    """Word error rate: edits to turn what was heard into what was said, over the words said."""
    a, b = words(said), words(heard)
    d = list(range(len(b) + 1))
    for i in range(1, len(a) + 1):
        prev, d[0] = d[0], i
        for j in range(1, len(b) + 1):
            cur = min(d[j] + 1, d[j - 1] + 1, prev + (a[i - 1] != b[j - 1]))
            prev, d[j] = d[j], cur
    return d[len(b)] / max(1, len(a))


def read_wav(path):
    with wave.open(path) as w:
        frames = w.readframes(w.getnframes())
        samples = np.frombuffer(frames, dtype=np.int16).astype(np.float32) / 32768.0
        if w.getnchannels() > 1:
            samples = samples.reshape(-1, w.getnchannels()).mean(axis=1)
        return samples, w.getframerate()


def resample(samples, rate, to=16000):
    """By the Fourier transform: enough for measuring, not for a product."""
    if rate == to:
        return samples
    n = int(round(len(samples) * to / rate))
    spectrum = np.fft.rfft(samples)
    keep = n // 2 + 1
    spectrum = spectrum[:keep] if len(spectrum) >= keep else np.pad(spectrum, (0, keep - len(spectrum)))
    return (np.fft.irfft(spectrum, n) * (n / len(samples))).astype(np.float32)


def find(folder, *patterns):
    for p in patterns:
        hits = sorted(glob.glob(os.path.join(M, folder, p)))
        if hits:
            return hits[0]
    raise SystemExit(f"Not found in {folder}: {patterns}. Run core/dev/voice-trial-setup.sh")


# ---------- speech out ----------
def kokoro(threads):
    d = os.path.join(M, "kokoro-int8-multi-lang-v1_0")
    lexicons = os.path.join(d, "lexicon-us-en.txt")
    config = sherpa_onnx.OfflineTtsConfig(
        model=sherpa_onnx.OfflineTtsModelConfig(
            kokoro=sherpa_onnx.OfflineTtsKokoroModelConfig(model=find("kokoro-int8-multi-lang-v1_0", "model*.onnx"), voices=os.path.join(d, "voices.bin"), tokens=os.path.join(d, "tokens.txt"),
                                                           data_dir=os.path.join(d, "espeak-ng-data"), dict_dir=os.path.join(d, "dict"), lexicon=lexicons, lang="en-us"),
            num_threads=threads, provider="cpu"),
        max_num_sentences=1)
    return sherpa_onnx.OfflineTts(config)


POCKET = "sherpa-onnx-pocket-tts-int8-2026-01-26"
POCKET_VOICES = ["bria.wav", "loona.wav"]  # the voices its package ships: recordings from Kyutai's own repository


def pocket(threads):
    d = os.path.join(M, POCKET)
    model = sherpa_onnx.OfflineTtsPocketModelConfig(lm_flow=os.path.join(d, "lm_flow.int8.onnx"), lm_main=os.path.join(d, "lm_main.int8.onnx"), encoder=os.path.join(d, "encoder.onnx"),
                                                    decoder=os.path.join(d, "decoder.int8.onnx"), text_conditioner=os.path.join(d, "text_conditioner.onnx"),
                                                    vocab_json=os.path.join(d, "vocab.json"), token_scores_json=os.path.join(d, "token_scores.json"))
    return sherpa_onnx.OfflineTts(sherpa_onnx.OfflineTtsConfig(model=sherpa_onnx.OfflineTtsModelConfig(pocket=model, num_threads=threads)))


def streamed(tts, text, voice):
    """Speech for a text in a voice, and how long until its first sound and until all of it."""
    g = sherpa_onnx.GenerationConfig()
    g.reference_audio, g.reference_sample_rate = voice
    g.speed = 1.0
    first = []
    started = time.perf_counter()

    def heard(samples, progress):
        if not first:
            first.append(time.perf_counter() - started)
        return 1

    audio = tts.generate(text, g, heard)
    whole = time.perf_counter() - started
    return np.asarray(audio.samples, dtype=np.float32), audio.sample_rate, (first[0] if first else whole), whole


# ---------- speech to text ----------
def parakeet(threads):
    f = "sherpa-onnx-nemo-parakeet-tdt-0.6b-v3-int8"
    return sherpa_onnx.OfflineRecognizer.from_transducer(encoder=find(f, "encoder*.onnx"), decoder=find(f, "decoder*.onnx"), joiner=find(f, "joiner*.onnx"), tokens=find(f, "tokens.txt"),
                                                         num_threads=threads, model_type="nemo_transducer", decoding_method="greedy_search")


def whisper(threads):
    f = "sherpa-onnx-whisper-turbo"
    return sherpa_onnx.OfflineRecognizer.from_whisper(encoder=find(f, "*encoder.int8.onnx", "*encoder.onnx"), decoder=find(f, "*decoder.int8.onnx", "*decoder.onnx"), tokens=find(f, "*tokens.txt"),
                                                      language="en", task="transcribe", num_threads=threads)


def hear(recognizer, samples, rate):
    stream = recognizer.create_stream()
    stream.accept_waveform(rate, samples)
    started = time.perf_counter()
    recognizer.decode_stream(stream)
    return stream.result.text.strip(), time.perf_counter() - started


# ---------- has the sentence ended? ----------
class SmartTurn:
    def __init__(self):
        options = ort.SessionOptions()
        options.execution_mode = ort.ExecutionMode.ORT_SEQUENTIAL
        options.inter_op_num_threads = 1
        options.intra_op_num_threads = THREADS
        options.graph_optimization_level = ort.GraphOptimizationLevel.ORT_ENABLE_ALL
        self.session = ort.InferenceSession(os.path.join(M, "smart-turn-v3.2-cpu.onnx"), sess_options=options, providers=["CPUExecutionProvider"])
        self.features = WhisperFeatureExtractor(chunk_length=8)

    def ended(self, samples16):
        """The probability that the speaker has finished, from the last 8 seconds, as Smart Turn's own code asks it:
        cut to the last 8 seconds, or padded with silence at the beginning to 8."""
        audio = samples16[-8 * 16000:]
        audio = np.pad(audio, (8 * 16000 - len(audio), 0))
        started = time.perf_counter()
        f = self.features(audio, sampling_rate=16000, return_tensors="np", padding="max_length", max_length=8 * 16000, truncation=True, do_normalize=True)
        x = f.input_features.squeeze(0).astype(np.float32)[None, ...]
        p = float(self.session.run(None, {"input_features": x})[0].reshape(-1)[0])
        return p, time.perf_counter() - started


def on_its_own_test_set(turn):
    """Smart Turn on recordings from its makers' test set (pipecat-ai/smart-turn-data-v3.1-test, CC-BY-4.0), each
    labelled finished or not: one shard of nine, if it is in the cache. Their report gives 94.3% right on English."""
    shard = os.path.join(CACHE, "smart-turn-test", "train-00000-of-00009.parquet")
    if not os.path.exists(shard):
        return None
    import io
    import pyarrow.parquet as pq
    import soundfile
    table = pq.read_table(shard, columns=["audio", "language", "endpoint_bool"]).to_pylist()
    by_language = {}
    for row in table:
        samples, rate = soundfile.read(io.BytesIO(row["audio"]["bytes"]), dtype="float32")
        if samples.ndim > 1:
            samples = samples.mean(axis=1)
        p, _ = turn.ended(resample(samples, rate))
        right = (p > 0.5) == bool(row["endpoint_bool"])
        lang = row["language"] or "?"
        seen = by_language.setdefault(lang, [0, 0, 0, 0])  # right, cases, finished said unfinished, unfinished said finished
        seen[0] += right
        seen[1] += 1
        seen[2] += bool(row["endpoint_bool"]) and p <= 0.5
        seen[3] += (not row["endpoint_bool"]) and p > 0.5
    return by_language


def as_after_a_pause(samples16, pause=0.2):
    """The speech as Smart Turn is given it: its own silence at the end taken off, then the pause the speech
    detector waits for before it asks whether the sentence is over."""
    loud = np.nonzero(np.abs(samples16) > 0.01)[0]
    end = loud[-1] + 1 if len(loud) else len(samples16)
    return np.concatenate([samples16[:end], np.zeros(int(pause * 16000), dtype=np.float32)])


def first_sentence_of_an_answer():
    """Qwen3 4B, through the runtime Demido installed: how long until its answer's first sentence is written."""
    home = os.environ.get("DEMIDO_HOME") or os.path.expanduser("~/.local/share/demido")
    notes = []
    for path in glob.glob(os.path.join(home, "runtimes/llama.cpp/*/demido-runtime.json")):
        note = json.load(open(path))
        if note.get("server") and note.get("version"):
            notes.append((note["installed_at"], os.path.dirname(path), note))
    if not notes:
        return None
    _, root, note = max(notes, key=lambda n: n[0])
    rt = os.path.dirname(os.path.join(root, note["server"]))
    libs = ":".join(os.path.join(root, d) for d in note.get("lib_dirs", []))
    work, port, key = tempfile.mkdtemp(prefix="demido-voice-"), 7467, secrets.token_hex(24)
    keyfile = os.path.join(work, "key.txt")
    open(keyfile, "w").write(key + "\n")
    os.chmod(keyfile, 0o600)
    model = "Qwen3-4B-Q4_K_M"
    router = subprocess.Popen([os.path.join(rt, "llama-server"), "--models-dir", os.path.join(home, "models"), "--host", "127.0.0.1", "--port", str(port), "--api-key-file", keyfile, "--no-models-autoload",
                               "--no-webui", "--models-max", "0", "--models-memory-margin", "900", "-c", "4096"], cwd=rt, env=dict(os.environ, LD_LIBRARY_PATH=libs), stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    base = f"http://127.0.0.1:{port}"
    call = lambda path, body=None: urllib.request.urlopen(urllib.request.Request(base + path, data=None if body is None else json.dumps(body).encode(), headers={"Authorization": f"Bearer {key}", "Content-Type": "application/json"}), timeout=300)
    try:
        for _ in range(200):
            try:
                if urllib.request.urlopen(base + "/health", timeout=1).status == 200:
                    break
            except Exception:
                time.sleep(0.05)
        call("/models/load", {"model": model})
        for _ in range(600):
            if any(m["id"] == model and m["status"]["value"] == "loaded" for m in json.load(call("/models"))["data"]):
                break
            time.sleep(0.2)
        times = []
        for question in COMPLETE[:6]:
            body = {"model": model, "stream": True, "temperature": 0, "chat_template_kwargs": {"enable_thinking": False}, "max_tokens": 80,
                    "messages": [{"role": "system", "content": "You are a voice assistant. Answer in one or two short spoken sentences."}, {"role": "user", "content": question}]}
            started, text, first = time.perf_counter(), "", None
            with call("/v1/chat/completions", body) as r:
                for line in r:
                    line = line.decode().strip()
                    if not line.startswith("data:") or line.endswith("[DONE]"):
                        continue
                    piece = json.loads(line[5:])["choices"][0]["delta"].get("content") or ""
                    text += piece
                    if first is None and re.search(r"[.!?](\s|$)", text):
                        first = time.perf_counter() - started
            times.append(first if first is not None else time.perf_counter() - started)
        return model, times
    finally:
        router.send_signal(signal.SIGTERM)
        try:
            router.wait(timeout=15)
        except subprocess.TimeoutExpired:
            router.kill()


def main():
    print(f"sherpa-onnx {getattr(sherpa_onnx, '__version__', '?')}, onnxruntime {ort.__version__}, {THREADS} processor threads; {os.cpu_count()} on this computer\n")

    # Speech out. Kokoro makes a sentence whole, so its first sound is the whole sentence's time.
    started = time.perf_counter()
    tts = kokoro(THREADS)
    print(f"Kokoro v1.0 loaded in {say_time(time.perf_counter() - started)}")
    made = []
    for sid in VOICES:
        for text in COMPLETE:
            t0 = time.perf_counter()
            audio = tts.generate(text, sid=sid, speed=1.0)
            made.append((time.perf_counter() - t0, len(audio.samples) / audio.sample_rate))
    took, length = np.array([m[0] for m in made]), np.array([m[1] for m in made])
    print(f"    {len(made)} sentences: first sound after median {say_time(np.median(took))}, slowest {say_time(took.max())}; {took.sum() / length.sum():.2f} s of work per second of speech "
          f"(words not in its lexicon are skipped, not spoken)")
    del tts
    # Pocket TTS streams: its first sound comes before the sentence is done. Each voice is read once first.
    started = time.perf_counter()
    tts = pocket(THREADS)
    print(f"Pocket TTS loaded in {say_time(time.perf_counter() - started)}")
    spoken, firsts, wholes, lengths = {}, [], [], []
    for name in POCKET_VOICES:
        voice_samples, voice_rate = read_wav(os.path.join(M, POCKET, "test_wavs", name))
        voice = (voice_samples.tolist(), voice_rate)
        _, _, warm, _ = streamed(tts, "Hello.", voice)
        for text in COMPLETE + SHORT:
            samples, rate, first, whole = streamed(tts, text, voice)
            spoken[(name, text)] = samples
            firsts.append(first)
            wholes.append(whole)
            lengths.append(len(samples) / rate)
        print(f"    voice {name}: read in {say_time(warm)} the first time")
    SPOKEN_RATE[0] = rate
    firsts, wholes, lengths = np.array(firsts), np.array(wholes), np.array(lengths)
    print(f"    {len(firsts)} sentences: first sound after median {say_time(np.median(firsts))}, slowest {say_time(firsts.max())}; whole sentence median {say_time(np.median(wholes))}; "
          f"{wholes.sum() / lengths.sum():.2f} s of work per second of speech\n")

    # Speech to text, on real speech and on what Pocket TTS said.
    real = sorted(set(glob.glob(os.path.join(M, "*", "test_wavs", "*.wav"))))
    for listed in glob.glob(os.path.join(M, "*", "test_wavs", "trans.txt")):
        for line in open(listed):
            name, _, said = line.strip().partition(" ")
            TRANSCRIPTS[os.path.join(os.path.dirname(listed), name)] = said
    for name, make in (("Parakeet TDT 0.6B v3", parakeet), ("Whisper large-v3-turbo", whisper)):
        t0 = time.perf_counter()
        recognizer = make(THREADS)
        loaded = time.perf_counter() - t0
        errors, times, seconds = [], [], []
        for (sid, text), samples in spoken.items():
            if text not in COMPLETE:
                continue
            heard, took = hear(recognizer, samples, SPOKEN_RATE[0])
            errors.append(wer(text, heard))
            times.append(took)
            seconds.append(len(samples) / SPOKEN_RATE[0])
            if SHOW:
                print(f"    [{name[:8]} {sid}] {text!r} -> {heard!r}")
        real_times, real_seconds, real_errors = [], [], []
        for path in real:
            samples, rate = read_wav(path)
            heard, took = hear(recognizer, samples, rate)
            real_times.append(took)
            real_seconds.append(len(samples) / rate)
            said = TRANSCRIPTS.get(path)
            if said:
                real_errors.append(wer(said, heard))
            if SHOW:
                print(f"    [{name[:8]} real] {os.path.basename(os.path.dirname(os.path.dirname(path)))}/{os.path.basename(path)} -> {heard!r}")
        times, seconds = np.array(times), np.array(seconds)
        print(f"{name}: loaded in {say_time(loaded)}. On {len(times)} spoken sentences ({seconds.mean():.1f} s each): median {say_time(np.median(times))}, slowest {say_time(times.max())}; "
              f"word errors {100 * np.mean(errors):.1f}%, sentences with any error {sum(e > 0 for e in errors)} of {len(errors)}")
        if real_times:
            scored = f"; word errors {100 * np.mean(real_errors):.1f}% on the {len(real_errors)} in English with a transcript" if real_errors else ""
            print(f"    on {len(real_times)} real recordings ({np.mean(real_seconds):.1f} s each): median {say_time(np.median(real_times))}, slowest {say_time(max(real_times))}{scored}")
        del recognizer

    # Has the sentence ended? Finished sentences, ones that stop short, and finished ones cut in the middle.
    turn = SmartTurn()
    rows, times = [], []
    for (sid, text), samples in spoken.items():
        s16 = resample(samples, SPOKEN_RATE[0])
        p, took = turn.ended(as_after_a_pause(s16))
        times.append(took)
        rows.append(("finished" if text in COMPLETE else "stops short", p))
        if text in COMPLETE:
            cut = s16[: int(len(as_after_a_pause(s16, 0)) * 0.55)]
            p, took = turn.ended(as_after_a_pause(cut))
            times.append(took)
            rows.append(("cut in the middle", p))
    for path in real:
        samples, rate = read_wav(path)
        p, took = turn.ended(as_after_a_pause(resample(samples, rate)))
        times.append(took)
        rows.append(("real, finished", p))
    print(f"\nSmart Turn v3.2: median {say_time(np.median(times))} a judgement (features and model), slowest {say_time(max(times))}")
    for kind in ("finished", "real, finished", "stops short", "cut in the middle"):
        ps = [p for k, p in rows if k == kind]
        if not ps:
            continue
        said_done = sum(p > 0.5 for p in ps)
        print(f"    {kind:18} {len(ps):3} cases: judged finished {said_done:3}  (median probability {np.median(ps):.2f})")

    own = on_its_own_test_set(turn)
    if own:
        right, cases = sum(v[0] for v in own.values()), sum(v[1] for v in own.values())
        en = own.get("eng") or own.get("en") or [0, 0, 0, 0]
        print(f"    on {cases} recordings of its own test set (one shard of nine): {100 * right / cases:.1f}% right; English {en[0]} of {en[1]} ({100 * en[0] / max(1, en[1]):.1f}%), "
              f"of them finished taken for unfinished {en[2]}, unfinished taken for finished {en[3]}")

    if os.environ.get("TRIAL_LLM"):
        got = first_sentence_of_an_answer()
        if got:
            model, firsts = got
            print(f"\n{model}, on the graphics card: its answer's first sentence after median {say_time(np.median(firsts))}, slowest {say_time(max(firsts))} ({len(firsts)} questions)")


if __name__ == "__main__":
    main()
