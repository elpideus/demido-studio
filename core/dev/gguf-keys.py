"""Prints a GGUF file's metadata keys (not the tokenizer's long lists), to see what its makers put in it."""
import struct, sys

def read(f, fmt):
    size = struct.calcsize(fmt)
    return struct.unpack("<" + fmt, f.read(size))[0]

def string(f):
    n = read(f, "Q")
    return f.read(n).decode("utf-8", "replace")

SCALAR = {0: "B", 1: "b", 2: "H", 3: "h", 4: "I", 5: "i", 6: "f", 7: "?", 10: "Q", 11: "q", 12: "d"}

def value(f, t):
    if t in SCALAR:
        return read(f, SCALAR[t])
    if t == 8:
        return string(f)
    if t == 9:
        inner, count = read(f, "I"), read(f, "Q")
        items = [value(f, inner) for _ in range(count)]
        return f"[{count} items]" if count > 8 else items
    raise ValueError(f"type {t}")

for path in sys.argv[1:]:
    with open(path, "rb") as f:
        assert f.read(4) == b"GGUF"
        version, tensors, kvs = read(f, "I"), read(f, "Q"), read(f, "Q")
        print(f"== {path.rsplit('/', 1)[-1]} (GGUF v{version}, {kvs} keys)")
        for _ in range(kvs):
            key = string(f)
            t = read(f, "I")
            v = value(f, t)
            if key.startswith("tokenizer.ggml.") and key not in ("tokenizer.ggml.model", "tokenizer.ggml.pre"):
                continue
            if key == "tokenizer.chat_template":
                v = f"<{len(v)} characters>"
            print(f"  {key} = {v}")
