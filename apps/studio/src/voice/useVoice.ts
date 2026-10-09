// The composer's microphone: records, then hands the recording on the way the model takes it.
// A model that hears gets a voice note; any other gets the words, written down by the speech
// model and typed into the composer, never sent by themselves.

import { useCallback, useEffect, useRef, useState } from 'react';
import { formatBytes } from '@demido/ui';

import { api, errorText } from '@/lib/api';
import { on } from '@/lib/events';
import type { ModelEntry, VoiceStatus } from '@/lib/types';
import { useApp } from '@/stores/app';
import { toast } from '@/stores/toasts';
import { MicError, Recorder } from './recorder';
import { MAX_FIT_SECONDS } from './wav';

export type VoicePhase = 'idle' | 'starting' | 'recording' | 'saving' | 'writing';

/** Why the microphone did not work, in plain words, with what the person can do about it. */
export interface VoiceProblem {
  text: string;
  action?: { label: string; run: () => void };
}

export interface VoiceHandlers {
  /** A recording for a model that hears, to attach as a voice note. */
  onNote: (wav: Uint8Array) => void;
  /** Words are about to be written into the composer: it notes where its caret is. */
  onDictationStart: () => void;
  /** The words written down so far. */
  onDictation: (words: string) => void;
  /** Writing down ended; `kept` is false when it was stopped, and the text goes back as it was. */
  onDictationEnd: (kept: boolean) => void;
}

// Shorter is a tap on the button, not something said.
const SHORTEST_SECONDS = 0.4;

const openWindowsSettings = {
  label: 'Open Windows settings',
  run: () => void api.openMicrophoneSettings().catch((e) => toast.error('Could not open Settings', errorText(e))),
};

const blockedProblem: VoiceProblem = {
  text: 'Windows is keeping apps from the microphone. Turn on “Microphone access” and “Let desktop apps access your microphone” in Windows Settings, then try again.',
  action: openWindowsSettings,
};

export function useVoice(chatId: string | null, model: ModelEntry | undefined, handlers: VoiceHandlers) {
  const [phase, setPhase] = useState<VoicePhase>('idle');
  const [meter, setMeter] = useState({ level: 0, seconds: 0 });
  const [limit, setLimit] = useState(MAX_FIT_SECONDS);
  const [problem, setProblem] = useState<VoiceProblem | null>(null);
  const recorder = useRef<Recorder | null>(null);
  const job = useRef<string | null>(null);
  // Set when the person cancels while recording starts.
  const abandoned = useRef(false);
  const latest = useRef({ handlers, model, chatId });
  latest.current = { handlers, model, chatId };

  /** The speech model is not on this computer: what to say, and the download to offer. */
  const missingSpeech = useCallback((status: VoiceStatus, name: string): VoiceProblem => {
    const m = status.missing;
    if (status.downloading || !m) {
      return {
        text: `The speech model is still downloading. Once it is done, what you say is written down for ${name}.`,
      };
    }
    return {
      text: `${name} can’t hear, so what you say is written down for it first, by a speech model that runs on this computer: ${m.name} (${formatBytes(m.size)}).`,
      action: {
        label: `Download ${m.name}`,
        run: () =>
          void api.downloadSpeechModel().then(
            () =>
              setProblem({
                text: `${m.name} is downloading; Settings, Models shows how far it got. The microphone works once it is done.`,
              }),
            (e) => setProblem({ text: `${m.name} could not be downloaded: ${errorText(e)}` }),
          ),
      },
    };
  }, []);

  const stop = useCallback(async () => {
    const rec = recorder.current;
    if (!rec) return;
    recorder.current = null;
    setPhase('saving');
    const { wav, seconds } = await rec.stop();
    if (abandoned.current) return;
    const { handlers, model, chatId } = latest.current;
    if (seconds < SHORTEST_SECONDS || !model) {
      setPhase('idle');
      return;
    }
    // The model may have been switched while recording: it decides where the recording goes.
    let status: VoiceStatus;
    try {
      status = await api.voiceStatus(model.id);
    } catch (e) {
      setProblem({ text: `The recording could not be used: ${errorText(e)}` });
      setPhase('idle');
      return;
    }
    if (status.route === 'audio') {
      handlers.onNote(wav);
      setPhase('idle');
      return;
    }
    if (!status.speech) {
      setProblem(missingSpeech(status, model.name));
      setPhase('idle');
      return;
    }
    const id = crypto.randomUUID();
    job.current = id;
    setPhase('writing');
    handlers.onDictationStart();
    const unlisten = await on('voice://text', (e) => {
      if (e.job === id && job.current === id) latest.current.handlers.onDictation(e.text);
    });
    try {
      const result = await api.transcribeRecording(wav, id, chatId);
      if (job.current !== id) return;
      latest.current.handlers.onDictation(result.text);
      latest.current.handlers.onDictationEnd(true);
      if (!result.text.trim()) setProblem({ text: 'No speech was heard in the recording.' });
    } catch (e) {
      if (job.current !== id) return;
      latest.current.handlers.onDictationEnd(false);
      setProblem({ text: errorText(e) });
    } finally {
      unlisten();
      if (job.current === id) {
        job.current = null;
        setPhase('idle');
      }
    }
  }, [missingSpeech]);

  const start = useCallback(async () => {
    const { model } = latest.current;
    if (!model || recorder.current) return;
    setProblem(null);
    setPhase('starting');
    abandoned.current = false;
    try {
      // Starts loading the speech model now, when it will be needed, so it is ready by the end.
      const status = await api.voiceStatus(model.id, true);
      if (status.microphoneBlocked) {
        setProblem(blockedProblem);
        setPhase('idle');
        return;
      }
      if (status.route === 'transcript' && !status.speech) {
        setProblem(missingSpeech(status, model.name));
        setPhase('idle');
        return;
      }
      const max = Math.min(status.maxSeconds, MAX_FIT_SECONDS);
      setLimit(max);
      setMeter({ level: 0, seconds: 0 });
      const rec = await Recorder.start({
        deviceId: useApp.getState().settings?.microphone ?? null,
        onLevel: (level, seconds) => {
          setMeter({ level, seconds });
          if (seconds >= max) void stop();
        },
      });
      if (abandoned.current) {
        rec.cancel();
        setPhase('idle');
        return;
      }
      recorder.current = rec;
      setPhase('recording');
    } catch (e) {
      setPhase('idle');
      if (!(e instanceof MicError)) {
        setProblem({ text: `Recording could not start: ${errorText(e)}` });
      } else if (e.problem === 'none') {
        setProblem({
          text: 'No microphone was found. Plug one in, or choose another in Settings, General, Voice.',
        });
      } else if (e.problem === 'denied') {
        setProblem({
          text: 'The microphone was refused. Windows may be keeping apps from it: check “Microphone access” in Windows Settings.',
          action: openWindowsSettings,
        });
      } else {
        setProblem({ text: e.message });
      }
    }
  }, [missingSpeech, stop]);

  /** Throws the recording away, or stops writing it down and puts the text back. */
  const cancel = useCallback(() => {
    abandoned.current = true;
    recorder.current?.cancel();
    recorder.current = null;
    const id = job.current;
    if (id) {
      job.current = null;
      void api.cancelTranscription(id).catch(() => undefined);
      latest.current.handlers.onDictationEnd(false);
    }
    setPhase('idle');
  }, []);

  // Escape cancels whatever the microphone is doing.
  const busy = phase !== 'idle';
  useEffect(() => {
    if (!busy) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== 'Escape') return;
      e.preventDefault();
      e.stopPropagation();
      cancel();
    };
    window.addEventListener('keydown', onKey, true);
    return () => window.removeEventListener('keydown', onKey, true);
  }, [busy, cancel]);

  // Nothing keeps recording once the composer is gone.
  useEffect(
    () => () => {
      recorder.current?.cancel();
      if (job.current) void api.cancelTranscription(job.current).catch(() => undefined);
    },
    [],
  );

  return { phase, meter, limit, problem, dismiss: () => setProblem(null), start, stop, cancel };
}
