// Records the microphone into the WAV the backend takes (see wav.ts).

import workletUrl from './capture.worklet.js?url';
import { level, recordingWav } from './wav';

/** Why recording could not start, in words the composer shows. */
export type MicProblem = 'none' | 'denied' | 'busy' | 'unsupported';

export class MicError extends Error {
  constructor(
    readonly problem: MicProblem,
    message: string,
  ) {
    super(message);
  }
}

export interface Recording {
  wav: Uint8Array;
  seconds: number;
}

interface Options {
  /** A device id from Settings; the system's default when null or no longer there. */
  deviceId: string | null;
  /** Called about ten times a second with the loudness (0 to 1) and the seconds recorded. */
  onLevel: (level: number, seconds: number) => void;
}

/** The microphone stream, asked for as speech models like it: one channel, cleaned up. */
async function microphone(deviceId: string | null): Promise<MediaStream> {
  if (!navigator.mediaDevices?.getUserMedia) throw new MicError('unsupported', 'This window cannot record sound.');
  const audio: MediaTrackConstraints = {
    channelCount: 1,
    echoCancellation: true,
    noiseSuppression: true,
    autoGainControl: true,
  };
  try {
    try {
      return await navigator.mediaDevices.getUserMedia({
        audio: deviceId ? { ...audio, deviceId: { exact: deviceId } } : audio,
      });
    } catch (e) {
      // The chosen microphone was unplugged: the default one will do.
      if (deviceId && e instanceof DOMException && (e.name === 'OverconstrainedError' || e.name === 'NotFoundError')) {
        return await navigator.mediaDevices.getUserMedia({ audio });
      }
      throw e;
    }
  } catch (e) {
    const name = e instanceof DOMException ? e.name : '';
    if (name === 'NotFoundError' || name === 'OverconstrainedError') {
      throw new MicError('none', 'No microphone was found.');
    }
    if (name === 'NotAllowedError' || name === 'SecurityError') {
      throw new MicError('denied', 'Demido Studio was not allowed to use the microphone.');
    }
    if (name === 'NotReadableError' || name === 'AbortError') {
      throw new MicError('busy', 'The microphone could not be started. Another app may be using it.');
    }
    throw new MicError('unsupported', e instanceof Error ? e.message : String(e));
  }
}

/** A recording under way. */
export class Recorder {
  private chunks: Float32Array[] = [];
  private samples = 0;
  private timer: number | undefined;
  private recent = 0;
  private closed = false;

  private constructor(
    private readonly stream: MediaStream,
    private readonly context: AudioContext,
    private readonly node: AudioWorkletNode,
    private readonly source: MediaStreamAudioSourceNode,
    options: Options,
  ) {
    node.port.onmessage = (e: MessageEvent<Float32Array | string>) => {
      if (typeof e.data === 'string') return;
      this.chunks.push(e.data);
      this.samples += e.data.length;
      this.recent = Math.max(this.recent, level(e.data));
    };
    this.timer = window.setInterval(() => {
      options.onLevel(this.recent, this.seconds);
      this.recent *= 0.5;
    }, 100);
  }

  static async start(options: Options): Promise<Recorder> {
    const stream = await microphone(options.deviceId);
    let context: AudioContext | null = null;
    try {
      context = new AudioContext();
      await context.audioWorklet.addModule(workletUrl);
      const source = context.createMediaStreamSource(stream);
      const node = new AudioWorkletNode(context, 'demido-capture', {
        numberOfInputs: 1,
        numberOfOutputs: 1,
        channelCount: 1,
        channelCountMode: 'explicit',
        channelInterpretation: 'speakers',
      });
      source.connect(node);
      // Its output is silence; connected so the graph keeps pulling the microphone through it.
      node.connect(context.destination);
      if (context.state === 'suspended') await context.resume();
      return new Recorder(stream, context, node, source, options);
    } catch (e) {
      for (const t of stream.getTracks()) t.stop();
      void context?.close().catch(() => undefined);
      throw new MicError('unsupported', `Recording could not start: ${e instanceof Error ? e.message : String(e)}`);
    }
  }

  /** Seconds recorded so far. */
  get seconds(): number {
    return this.samples / this.context.sampleRate;
  }

  /** Ends the recording and returns it. */
  async stop(): Promise<Recording> {
    if (!this.closed) {
      // What the audio thread still holds comes after the last block it sent.
      await new Promise<void>((resolve) => {
        const done = window.setTimeout(resolve, 300);
        this.node.port.onmessage = (e: MessageEvent<Float32Array | string>) => {
          if (e.data === 'flushed') {
            window.clearTimeout(done);
            resolve();
          } else if (typeof e.data !== 'string') {
            this.chunks.push(e.data);
            this.samples += e.data.length;
          }
        };
        this.node.port.postMessage('flush');
      });
    }
    const rate = this.context.sampleRate;
    const seconds = this.seconds;
    this.close();
    return { wav: recordingWav(this.chunks, rate), seconds };
  }

  /** Ends the recording and throws it away. */
  cancel(): void {
    this.close();
    this.chunks = [];
  }

  private close(): void {
    if (this.closed) return;
    this.closed = true;
    window.clearInterval(this.timer);
    this.node.port.onmessage = null;
    this.source.disconnect();
    this.node.disconnect();
    for (const t of this.stream.getTracks()) t.stop();
    void this.context.close().catch(() => undefined);
  }
}

/** The microphones on this computer. Their names show once the app has used one. */
export async function microphones(): Promise<MediaDeviceInfo[]> {
  if (!navigator.mediaDevices?.enumerateDevices) return [];
  const devices = await navigator.mediaDevices.enumerateDevices();
  return devices.filter((d) => d.kind === 'audioinput' && d.deviceId !== 'default' && d.deviceId !== 'communications');
}

/** Opens the microphone for a moment, so the browser names the microphones. */
export async function unlockNames(): Promise<void> {
  const stream = await microphone(null);
  for (const t of stream.getTracks()) t.stop();
}
