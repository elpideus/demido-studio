import { describe, expect, it } from 'vitest';

import { MAX_BYTES, MAX_FIT_SECONDS, clock, downsample, encodeWav, level, recordingWav, toPcm16 } from './wav';

const ascii = (b: Uint8Array, at: number, n: number) => String.fromCharCode(...b.slice(at, at + n));

describe('wav', () => {
  it('writes a 16 kHz mono PCM16 header', () => {
    const wav = encodeWav(new Int16Array([1, -2, 3]));
    const view = new DataView(wav.buffer);
    expect(wav.length).toBe(44 + 6);
    expect(ascii(wav, 0, 4)).toBe('RIFF');
    expect(view.getUint32(4, true)).toBe(36 + 6);
    expect(ascii(wav, 8, 8)).toBe('WAVEfmt ');
    expect(view.getUint32(16, true)).toBe(16);
    expect(view.getUint16(20, true)).toBe(1);
    expect(view.getUint16(22, true)).toBe(1);
    expect(view.getUint32(24, true)).toBe(16_000);
    expect(view.getUint32(28, true)).toBe(32_000);
    expect(view.getUint16(32, true)).toBe(2);
    expect(view.getUint16(34, true)).toBe(16);
    expect(ascii(wav, 36, 4)).toBe('data');
    expect(view.getUint32(40, true)).toBe(6);
    expect([view.getInt16(44, true), view.getInt16(46, true), view.getInt16(48, true)]).toEqual([1, -2, 3]);
  });

  it('holds samples beyond full scale at it', () => {
    expect(Array.from(toPcm16(new Float32Array([0, 1, -1, 1.7, -3, 0.5, -0.5])))).toEqual([
      0, 32767, -32768, 32767, -32768, 16384, -16384,
    ]);
    expect(Array.from(toPcm16(new Float32Array([Number.NaN])))).toEqual([0]);
  });

  it('brings 48 kHz down to 16 kHz by averaging', () => {
    const input = new Float32Array([0.3, 0.3, 0.3, -0.6, 0, 0, 1, 1]);
    const out = downsample(input, 48_000);
    expect(out.length).toBe(2);
    expect(out[0]).toBeCloseTo(0.3);
    expect(out[1]).toBeCloseTo(-0.2);
    // 44.1 kHz: a ratio that is not whole still gives one sample per 1/16000 s.
    expect(downsample(new Float32Array(44_100), 44_100).length).toBe(16_000);
    expect(downsample(input, 16_000)).toBe(input);
  });

  it('joins chunks into one recording', () => {
    const wav = recordingWav([new Float32Array(48_000).fill(0.5), new Float32Array(48_000)], 48_000);
    expect(wav.length).toBe(44 + 32_000 * 2);
    expect(new DataView(wav.buffer).getInt16(44, true)).toBe(16_384);
    expect(new DataView(wav.buffer).getInt16(wav.length - 2, true)).toBe(0);
  });

  it('fits a little over five minutes in the size limit', () => {
    expect(MAX_FIT_SECONDS).toBe(327);
    expect(44 + MAX_FIT_SECONDS * 32_000).toBeLessThanOrEqual(MAX_BYTES);
  });

  it('measures loudness and shows time', () => {
    expect(level(new Float32Array(100))).toBe(0);
    expect(level(new Float32Array(100).fill(1))).toBe(1);
    expect(level(new Float32Array(100).fill(0.01))).toBeCloseTo(0.4);
    expect(clock(0)).toBe('0:00');
    expect(clock(65.9)).toBe('1:05');
    expect(clock(300)).toBe('5:00');
  });
});
