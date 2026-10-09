// A recording as the backend takes it: 16 kHz mono PCM16 WAV, what speech models read.

/** Samples per second of every recording sent. */
export const SAMPLE_RATE = 16_000;

/** The largest recording sent, header included (the backend refuses more). */
export const MAX_BYTES = 10 * 1024 * 1024;

const HEADER_BYTES = 44;

/** Seconds of recording that fit in `MAX_BYTES`: a little over five minutes. */
export const MAX_FIT_SECONDS = Math.floor((MAX_BYTES - HEADER_BYTES) / (SAMPLE_RATE * 2));

/**
 * `input`, recorded at `fromRate`, at `toRate`. Each output sample is the mean of the input
 * samples it stands for, which keeps what lies above the new rate's limit from folding back
 * into the speech as noise.
 */
export function downsample(input: Float32Array, fromRate: number, toRate = SAMPLE_RATE): Float32Array {
  if (fromRate === toRate) return input;
  if (fromRate < toRate) throw new Error(`Cannot raise ${fromRate} Hz to ${toRate} Hz.`);
  const ratio = fromRate / toRate;
  const length = Math.floor(input.length / ratio);
  const output = new Float32Array(length);
  for (let i = 0; i < length; i++) {
    const start = Math.floor(i * ratio);
    const end = Math.min(input.length, Math.floor((i + 1) * ratio));
    let sum = 0;
    for (let j = start; j < end; j++) sum += input[j]!;
    output[i] = end > start ? sum / (end - start) : 0;
  }
  return output;
}

/** Float samples as 16-bit integers; anything beyond full scale is held at it. */
export function toPcm16(samples: Float32Array): Int16Array {
  const out = new Int16Array(samples.length);
  for (let i = 0; i < samples.length; i++) {
    const s = Math.max(-1, Math.min(1, samples[i]!));
    out[i] = s < 0 ? Math.round(s * 0x8000) : Math.round(s * 0x7fff);
  }
  return out;
}

/** A mono PCM16 WAV file of `samples` at `rate`. */
export function encodeWav(samples: Int16Array, rate = SAMPLE_RATE): Uint8Array {
  const bytes = new Uint8Array(HEADER_BYTES + samples.length * 2);
  const view = new DataView(bytes.buffer);
  const text = (at: number, s: string) => {
    for (let i = 0; i < s.length; i++) view.setUint8(at + i, s.charCodeAt(i));
  };
  text(0, 'RIFF');
  view.setUint32(4, 36 + samples.length * 2, true);
  text(8, 'WAVE');
  text(12, 'fmt ');
  view.setUint32(16, 16, true);
  view.setUint16(20, 1, true); // PCM
  view.setUint16(22, 1, true); // mono
  view.setUint32(24, rate, true);
  view.setUint32(28, rate * 2, true); // bytes per second
  view.setUint16(32, 2, true); // bytes per sample
  view.setUint16(34, 16, true); // bits per sample
  text(36, 'data');
  view.setUint32(40, samples.length * 2, true);
  for (let i = 0; i < samples.length; i++) view.setInt16(HEADER_BYTES + i * 2, samples[i]!, true);
  return bytes;
}

/** Joins recorded chunks, at `rate`, into the WAV sent. */
export function recordingWav(chunks: Float32Array[], rate: number): Uint8Array {
  const length = chunks.reduce((n, c) => n + c.length, 0);
  const all = new Float32Array(length);
  let at = 0;
  for (const c of chunks) {
    all.set(c, at);
    at += c.length;
  }
  return encodeWav(toPcm16(downsample(all, rate)));
}

/** The loudness of `samples` (0 to 1), for the level meter: RMS, scaled so speech fills it. */
export function level(samples: Float32Array): number {
  if (samples.length === 0) return 0;
  let sum = 0;
  for (let i = 0; i < samples.length; i++) sum += samples[i]! * samples[i]!;
  const rms = Math.sqrt(sum / samples.length);
  // Speech peaks near -10 dBFS and quiet rooms sit below -60: map -60..-10 dB onto 0..1.
  const db = 20 * Math.log10(Math.max(rms, 1e-6));
  return Math.max(0, Math.min(1, (db + 60) / 50));
}

/** "1:05" for 65 seconds. */
export function clock(seconds: number): string {
  const s = Math.max(0, Math.floor(seconds));
  return `${Math.floor(s / 60)}:${String(s % 60).padStart(2, '0')}`;
}
