import { useEffect, useRef, useState, type PointerEvent } from 'react';
import { ChevronDown, ChevronRight, Mic, Pause, Play } from 'lucide-react';
import { Spinner, cx } from '@demido/ui';

import { api, errorText } from '@/lib/api';
import { fileUrl } from '@/lib/format';
import type { Attachment } from '@/lib/types';
import { toast } from '@/stores/toasts';
import { clock } from '@/voice/wav';
import styles from './VoiceNote.module.css';

/** A voice note's player: play and pause, how far it got, and its length. */
export function VoicePlayer({ attachment, className }: { attachment: Attachment; className?: string }) {
  const audio = useRef<HTMLAudioElement>(null);
  const [playing, setPlaying] = useState(false);
  const [at, setAt] = useState(0);
  const length = (attachment.voice?.durationMs ?? 0) / 1000;
  const src = fileUrl(attachment.path);

  useEffect(() => {
    const el = audio.current;
    return () => el?.pause();
  }, []);

  const toggle = () => {
    const el = audio.current;
    if (!el) return;
    if (el.paused) {
      el.play().catch((e) => toast.error('Could not play the voice note', errorText(e)));
    } else {
      el.pause();
    }
  };

  const seek = (e: PointerEvent<HTMLDivElement>) => {
    const el = audio.current;
    if (!el || !length) return;
    const box = e.currentTarget.getBoundingClientRect();
    el.currentTime = Math.max(0, Math.min(1, (e.clientX - box.left) / box.width)) * length;
    setAt(el.currentTime);
  };

  const Icon = playing ? Pause : Play;
  return (
    <div className={cx(styles.player, className)}>
      <button
        type="button"
        className={styles.play}
        aria-label={playing ? 'Pause the voice note' : 'Play the voice note'}
        onClick={toggle}
        disabled={!src}
      >
        <Icon size={14} strokeWidth={2.2} aria-hidden />
      </button>
      <div
        className={styles.track}
        role="slider"
        aria-label="Position in the voice note"
        aria-valuemin={0}
        aria-valuemax={Math.round(length)}
        aria-valuenow={Math.round(at)}
        tabIndex={-1}
        onPointerDown={seek}
      >
        <div className={styles.fill} style={{ width: `${length ? Math.min(100, (at / length) * 100) : 0}%` }} />
      </div>
      <span className={styles.time}>{playing || at > 0 ? `${clock(at)} / ${clock(length)}` : clock(length)}</span>
      {src && (
        <audio
          ref={audio}
          src={src}
          preload="none"
          onPlay={() => setPlaying(true)}
          onPause={() => setPlaying(false)}
          onEnded={() => {
            setPlaying(false);
            setAt(0);
          }}
          onTimeUpdate={(e) => setAt(e.currentTarget.currentTime)}
        />
      )}
    </div>
  );
}

/** A voice note in the composer's tray, before it is sent. */
export function StagedVoiceNote({ attachment }: { attachment: Attachment | null }) {
  return (
    <div className={styles.staged}>
      <span className={styles.badge} aria-hidden>
        <Mic size={14} strokeWidth={2} />
      </span>
      {attachment ? (
        <VoicePlayer attachment={attachment} />
      ) : (
        <span className={styles.saving}>
          <Spinner size={12} /> Saving the recording…
        </span>
      )}
    </div>
  );
}

/** A voice note sent in a chat: its player, and what was said, written down, folded below. */
export function SentVoiceNote({ attachment }: { attachment: Attachment }) {
  const [open, setOpen] = useState(false);
  const [writing, setWriting] = useState(false);
  const transcript = attachment.voice?.transcript ?? null;

  const writeDown = () => {
    setWriting(true);
    setOpen(true);
    api
      .transcribeVoiceNote(attachment.id)
      .catch((e) => toast.error('Could not write down the voice note', errorText(e)))
      .finally(() => setWriting(false));
  };

  const Chevron = open ? ChevronDown : ChevronRight;
  return (
    <div className={styles.sent}>
      <VoicePlayer attachment={attachment} />
      {transcript === null ? (
        <button type="button" className={styles.disclose} onClick={writeDown} disabled={writing}>
          {writing ? <Spinner size={11} /> : <Chevron size={13} strokeWidth={2} aria-hidden />}
          {writing ? 'Writing it down…' : 'Show what was said'}
        </button>
      ) : (
        <>
          <button type="button" className={styles.disclose} aria-expanded={open} onClick={() => setOpen(!open)}>
            <Chevron size={13} strokeWidth={2} aria-hidden />
            Transcript
          </button>
          {open && (
            <div className={cx(styles.transcript, 'selectable')}>
              {transcript.trim() ? transcript : <em>No speech was heard in it.</em>}
              {attachment.voice?.transcribedBy && (
                <div className={styles.by}>Written down by {attachment.voice.transcribedBy}</div>
              )}
            </div>
          )}
        </>
      )}
    </div>
  );
}
