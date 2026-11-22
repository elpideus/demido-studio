#!/usr/bin/env python3
"""Talks to `demido voice-engine` the way the core does (frames on its standard input and output), with a WAV
file as the person: its sound, then silence; then one sentence for the engine to say. Prints each event, and
how much sound came back. For trying the engine by hand; the core's own tests use a stand-in.

    python3 core/dev/voice-engine-try.py <voice dir> <voice wav> <speech wav, 16 kHz mono 16-bit> ["a sentence to say"]"""
import struct, subprocess, sys, threading, time, wave, json, os

demido = os.path.join(os.path.dirname(__file__), "..", "target", "debug", "demido")
folder, voice, speech = sys.argv[1:4]
say = sys.argv[4] if len(sys.argv) > 4 else "It will be sunny in Rome tomorrow, with a high of nineteen degrees."
engine = subprocess.Popen([demido, "voice-engine", "--dir", folder, "--voice", voice], stdin=subprocess.PIPE, stdout=subprocess.PIPE)
started = time.perf_counter()


def send(kind, payload=b""):
    engine.stdin.write(bytes([kind]) + struct.pack("<I", len(payload)) + payload)
    engine.stdin.flush()


heard = threading.Event()
said = threading.Event()
sound = {}


def read():
    while True:
        head = engine.stdout.read(5)
        if len(head) < 5:
            return
        kind, n = head[0], struct.unpack("<I", head[1:])[0]
        payload = engine.stdout.read(n)
        if kind == ord("e"):
            event = json.loads(payload)
            print(f"{time.perf_counter() - started:6.2f} s  {event}", flush=True)
            if event["event"] in ("heard", "heard_nothing"):
                heard.set()
            if event["event"] == "said":
                said.set()
        elif kind == ord("o"):
            sentence = struct.unpack("<I", payload[:4])[0]
            sound[sentence] = sound.get(sentence, 0) + (len(payload) - 4) // 2


threading.Thread(target=read, daemon=True).start()
with wave.open(speech) as w:
    assert w.getframerate() == 16000 and w.getnchannels() == 1 and w.getsampwidth() == 2, "16 kHz mono 16-bit, please"
    pcm = w.readframes(w.getnframes())
time.sleep(4)  # the models load first
for i in range(0, len(pcm), 640):  # 20 ms at a time, as the window sends it
    send(ord("a"), pcm[i:i + 640])
for _ in range(50):  # one second of silence
    send(ord("a"), b"\0" * 640)
heard.wait(30)
send(ord("s"), struct.pack("<I", 1) + say.encode())
said.wait(60)
print(f"sound back for sentence 1: {sound.get(1, 0) / 24000:.2f} s")
send(ord("q"))
engine.wait(30)
