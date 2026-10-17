import { type KeyboardEvent, useEffect, useId, useRef, useState } from 'react';
import { ChevronUp, FileText, Minus, Paperclip, Send, X } from 'lucide-react';
import { Button, Dialog, IconButton, Notice, Select, Spinner, cx, formatBytes } from '@demido/ui';
import { open } from '@tauri-apps/plugin-dialog';

import { api, errorText } from '@/lib/api';
import type { MailAccount, MailAttachment, MailOutgoing, MailPickedFile, MailPrefill, MailStart } from '@/lib/types';
import { toast } from '@/stores/toasts';
import { MAX_ATTACHMENTS, splitAddresses } from './mailActions';
import { accountLabel } from './mailView';
import styles from './MailWindow.module.css';

/** A message to write: a new one, or a reply, forward or draft of message `id`. */
export interface ComposeRequest {
  /** New for each request, so the editor starts over. */
  key: number;
  account: string;
  start: MailStart | null;
  id: number | null;
}

interface Fields {
  account: string;
  to: string;
  cc: string;
  bcc: string;
  subject: string;
  text: string;
}

const TITLES: Record<MailStart, string> = {
  reply: 'Reply',
  replyAll: 'Reply all',
  forward: 'Forward',
  draft: 'Draft',
};

/** What the editor holds, to tell whether the person changed anything. */
function snapshot(fields: Fields, files: MailPickedFile[], carried: MailAttachment[]): string {
  return JSON.stringify([fields, files.map((f) => f.path), carried.map((a) => a.section)]);
}

/**
 * The message being written, floating over the bottom of the Mail window. A reply, forward or
 * draft starts from what the backend prepares from the original; what the person sees is what is
 * sent.
 */
export function Composer({
  request,
  accounts,
  raise,
  onDirty,
  onClose,
}: {
  request: ComposeRequest;
  accounts: MailAccount[];
  /** Changes when the window wants the editor shown again. */
  raise: number;
  onDirty: (dirty: boolean) => void;
  onClose: () => void;
}) {
  const id = useId();
  const blank: Fields = { account: request.account, to: '', cc: '', bcc: '', subject: '', text: '' };
  const [fields, setFields] = useState<Fields>(blank);
  const [prefill, setPrefill] = useState<MailPrefill | null>(null);
  const [carried, setCarried] = useState<MailAttachment[]>([]);
  const [files, setFiles] = useState<MailPickedFile[]>([]);
  const [initial, setInitial] = useState(() => snapshot(blank, [], []));
  const [loading, setLoading] = useState(request.id !== null);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [attempt, setAttempt] = useState(0);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState<'send' | 'draft' | null>(null);
  const [minimized, setMinimized] = useState(false);
  const [copies, setCopies] = useState(false);
  const [closing, setClosing] = useState(false);
  const toRef = useRef<HTMLInputElement>(null);
  const bodyRef = useRef<HTMLTextAreaElement>(null);

  useEffect(() => {
    if (request.id === null || request.start === null) return;
    let live = true;
    setLoading(true);
    setLoadError(null);
    api
      .mailPrefill(request.id, request.start)
      .then((p) => {
        if (!live) return;
        const next: Fields = {
          account: p.account,
          to: p.to.join(', '),
          cc: p.cc.join(', '),
          bcc: p.bcc.join(', '),
          subject: p.subject,
          text: p.text,
        };
        setFields(next);
        setPrefill(p);
        setCarried(p.attachments);
        setCopies(p.cc.length > 0 || p.bcc.length > 0);
        setInitial(snapshot(next, [], p.attachments));
        setLoading(false);
      })
      .catch((e) => {
        if (!live) return;
        setLoadError(errorText(e));
        setLoading(false);
      });
    return () => {
      live = false;
    };
  }, [request, attempt]);

  // A reply is written above the quote; a forward needs someone to go to.
  useEffect(() => {
    if (loading || loadError) return;
    const frame = requestAnimationFrame(() => {
      if (request.start === 'reply' || request.start === 'replyAll' || request.start === 'draft') {
        bodyRef.current?.focus();
        bodyRef.current?.setSelectionRange(0, 0);
        if (bodyRef.current) bodyRef.current.scrollTop = 0;
      } else {
        toRef.current?.focus();
      }
    });
    return () => cancelAnimationFrame(frame);
  }, [loading, loadError, request.start]);

  useEffect(() => {
    if (raise) setMinimized(false);
  }, [raise]);

  const dirty = !loading && snapshot(fields, files, carried) !== initial;
  useEffect(() => onDirty(dirty), [dirty, onDirty]);

  const set = (key: keyof Fields) => (e: { target: { value: string } }) => {
    const value = e.target.value;
    setFields((f) => ({ ...f, [key]: value }));
  };

  const total = files.reduce((n, f) => n + f.size, 0) + carried.reduce((n, a) => n + a.size, 0);
  const tooBig = total > MAX_ATTACHMENTS;
  const sending = busy !== null;

  const outgoing = (): MailOutgoing => ({
    account: fields.account,
    to: splitAddresses(fields.to),
    cc: splitAddresses(fields.cc),
    bcc: splitAddresses(fields.bcc),
    subject: fields.subject.trim(),
    text: fields.text,
    files: files.map((f) => f.path),
    carry:
      prefill?.attachmentsOf != null && carried.length
        ? { id: prefill.attachmentsOf, sections: carried.map((a) => a.section) }
        : null,
    replyTo: prefill?.replyTo ?? null,
    forward: prefill?.forward ?? null,
    inReplyTo: prefill?.inReplyTo ?? null,
    references: prefill?.references ?? [],
    draft: prefill?.draft ?? null,
  });

  const send = async () => {
    if (sending || loading || loadError || tooBig) return;
    const message = outgoing();
    if (!message.to.length && !message.cc.length && !message.bcc.length) {
      setError('Add at least one address to send the message to.');
      toRef.current?.focus();
      return;
    }
    setBusy('send');
    setError(null);
    try {
      const notes = await api.mailSend(message);
      toast.success('Message sent', message.subject || '(no subject)');
      for (const note of notes) toast.warning('The message was sent', note);
      onClose();
    } catch (e) {
      setError(errorText(e));
      setBusy(null);
    }
  };

  const saveDraft = async () => {
    if (sending || loading || loadError || tooBig) return;
    setBusy('draft');
    setError(null);
    try {
      const notes = await api.mailSaveDraft(outgoing());
      toast.success('Saved in Drafts', fields.subject.trim() || '(no subject)');
      for (const note of notes) toast.warning('The draft was saved', note);
      onClose();
    } catch (e) {
      setError(errorText(e));
      setBusy(null);
      setClosing(false);
    }
  };

  const attach = async () => {
    const picked = await open({ multiple: true, title: 'Attach files' });
    if (!picked) return;
    const paths = Array.isArray(picked) ? picked : [picked];
    try {
      const added = await api.mailAttachFiles(paths);
      setFiles((list) => [...list, ...added.filter((a) => !list.some((f) => f.path === a.path))]);
    } catch (e) {
      setError(errorText(e));
    }
  };

  const close = () => {
    if (sending) return;
    if (dirty) {
      setMinimized(false);
      setClosing(true);
    } else {
      onClose();
    }
  };

  const onKeyDown = (e: KeyboardEvent) => {
    // Not from the dialog asking whether to keep it, whose keys bubble up from its portal.
    if (closing || minimized) return;
    if (e.key === 'Enter' && (e.ctrlKey || e.metaKey)) {
      e.preventDefault();
      void send();
    }
  };

  const title = fields.subject.trim() || (request.start ? TITLES[request.start] : 'New message');
  const fromLocked = prefill?.draft != null;

  return (
    <section
      className={cx(styles.composer, minimized && styles.composerMin)}
      aria-label="Write a message"
      onKeyDown={onKeyDown}
    >
      <header
        className={styles.composerHead}
        onClick={() => minimized && setMinimized(false)}
        onDoubleClick={() => setMinimized((m) => !m)}
      >
        <span className={styles.composerTitle}>{title}</span>
        <IconButton
          icon={minimized ? ChevronUp : Minus}
          label={minimized ? 'Show' : 'Minimize'}
          size="xs"
          onClick={(e) => {
            e.stopPropagation();
            setMinimized((m) => !m);
          }}
        />
        <IconButton
          icon={X}
          label="Close"
          size="xs"
          disabled={sending}
          onClick={(e) => {
            e.stopPropagation();
            close();
          }}
        />
      </header>
      {!minimized &&
        (loading ? (
          <div className={styles.composerLoading}>
            <Spinner />
          </div>
        ) : loadError ? (
          <div className={styles.composerLoading}>
            <Notice
              tone="danger"
              action={
                <Button size="sm" variant="secondary" onClick={() => setAttempt((a) => a + 1)}>
                  Try again
                </Button>
              }
            >
              {loadError}
            </Notice>
          </div>
        ) : (
          <>
            <div className={styles.composerFields}>
              {accounts.length > 1 && (
                <div className={styles.composerLine}>
                  <label htmlFor={`${id}-from`}>From</label>
                  <Select
                    id={`${id}-from`}
                    size="sm"
                    className={styles.grow}
                    disabled={fromLocked}
                    options={accounts.map((a) => ({ value: a.id, label: accountLabel(a) }))}
                    value={fields.account}
                    onChange={set('account')}
                  />
                </div>
              )}
              <div className={styles.composerLine}>
                <label htmlFor={`${id}-to`}>To</label>
                <input
                  id={`${id}-to`}
                  ref={toRef}
                  className={styles.composerInput}
                  value={fields.to}
                  onChange={set('to')}
                  spellCheck={false}
                  autoComplete="off"
                />
                {!copies && (
                  <button type="button" className={styles.copiesToggle} onClick={() => setCopies(true)}>
                    Cc Bcc
                  </button>
                )}
              </div>
              {copies && (
                <>
                  <div className={styles.composerLine}>
                    <label htmlFor={`${id}-cc`}>Cc</label>
                    <input
                      id={`${id}-cc`}
                      className={styles.composerInput}
                      value={fields.cc}
                      onChange={set('cc')}
                      spellCheck={false}
                      autoComplete="off"
                    />
                  </div>
                  <div className={styles.composerLine}>
                    <label htmlFor={`${id}-bcc`}>Bcc</label>
                    <input
                      id={`${id}-bcc`}
                      className={styles.composerInput}
                      value={fields.bcc}
                      onChange={set('bcc')}
                      spellCheck={false}
                      autoComplete="off"
                    />
                  </div>
                </>
              )}
              <div className={styles.composerLine}>
                <input
                  aria-label="Subject"
                  placeholder="Subject"
                  className={styles.composerInput}
                  value={fields.subject}
                  onChange={set('subject')}
                />
              </div>
            </div>
            <textarea
              ref={bodyRef}
              aria-label="Message"
              className={styles.composerBody}
              value={fields.text}
              onChange={set('text')}
            />
            {(files.length > 0 || carried.length > 0) && (
              <div className={styles.composerFiles}>
                {carried.map((a) => (
                  <span key={`c${a.section}`} className={styles.fileChip} title={a.name}>
                    <FileText size={13} />
                    <span className={styles.attachmentName}>{a.name}</span>
                    <span className={styles.attachmentSize}>{formatBytes(a.size)}</span>
                    <IconButton
                      icon={X}
                      label={`Remove ${a.name}`}
                      size="xs"
                      tooltip={false}
                      onClick={() => setCarried((l) => l.filter((x) => x.section !== a.section))}
                    />
                  </span>
                ))}
                {files.map((f) => (
                  <span key={f.path} className={styles.fileChip} title={f.path}>
                    <Paperclip size={13} />
                    <span className={styles.attachmentName}>{f.name}</span>
                    <span className={styles.attachmentSize}>{formatBytes(f.size)}</span>
                    <IconButton
                      icon={X}
                      label={`Remove ${f.name}`}
                      size="xs"
                      tooltip={false}
                      onClick={() => setFiles((l) => l.filter((x) => x.path !== f.path))}
                    />
                  </span>
                ))}
              </div>
            )}
            {tooBig && (
              <Notice tone="warning" className={styles.composerNotice}>
                The attachments come to {formatBytes(total)}, more than the 25 MB mail servers take.
              </Notice>
            )}
            {error && (
              <Notice tone="danger" className={styles.composerNotice}>
                {error}
              </Notice>
            )}
            <footer className={styles.composerFoot}>
              <Button
                variant="primary"
                size="sm"
                icon={Send}
                loading={busy === 'send'}
                disabled={sending || tooBig}
                title="Send (Ctrl+Enter)"
                onClick={() => void send()}
              >
                Send
              </Button>
              <IconButton
                icon={Paperclip}
                label="Attach files"
                size="sm"
                disabled={sending}
                onClick={() => void attach()}
              />
              <span className={styles.grow} />
              <Button
                variant="ghost"
                size="sm"
                loading={busy === 'draft'}
                disabled={sending || tooBig}
                onClick={() => void saveDraft()}
              >
                Save draft
              </Button>
            </footer>
          </>
        ))}
      <Dialog
        open={closing}
        onClose={() => setClosing(false)}
        title="Keep this message?"
        description="Save it in Drafts to finish it later, or throw it away."
        width={420}
        footer={
          <>
            <Button
              variant="ghost"
              onClick={() => {
                setClosing(false);
                onClose();
              }}
            >
              Discard
            </Button>
            <Button variant="secondary" onClick={() => setClosing(false)}>
              Keep writing
            </Button>
            <Button variant="primary" loading={busy === 'draft'} disabled={tooBig} onClick={() => void saveDraft()}>
              Save draft
            </Button>
          </>
        }
      />
    </section>
  );
}
