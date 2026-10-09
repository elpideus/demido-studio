import { type KeyboardEvent, useId, useState } from 'react';
import { AtSign, ExternalLink, KeyRound, Server, Tag, User } from 'lucide-react';
import { Button, Field, Notice, SegmentedControl, TextField } from '@demido/ui';
import { openUrl } from '@tauri-apps/plugin-opener';

import { errorText } from '@/lib/api';
import type { MailAccount, MailKind } from '@/lib/types';
import { useMail } from '@/stores/mail';
import { MAX_NAME } from './NameForm';
import styles from './MailWindow.module.css';

const APP_PASSWORDS_URL = 'https://myaccount.google.com/apppasswords';

const KINDS = [
  { value: 'gmail' as const, label: 'Gmail' },
  { value: 'imap' as const, label: 'Other (IMAP)' },
];

/**
 * Connects a mail account, or signs one in again with a new password. The account is checked
 * by signing in before it is saved.
 */
export function ConnectForm({
  initial,
  onDone,
  onCancel,
}: {
  /** The account to sign in again. */
  initial?: MailAccount;
  onDone: (account: MailAccount) => void;
  onCancel?: () => void;
}) {
  const id = useId();
  const [kind, setKind] = useState<MailKind>(initial?.kind ?? 'gmail');
  const [email, setEmail] = useState(initial?.email ?? '');
  const [password, setPassword] = useState('');
  const [host, setHost] = useState(initial?.kind === 'imap' ? initial.host : '');
  const [port, setPort] = useState('993');
  const [username, setUsername] = useState('');
  const [nickname, setNickname] = useState('');
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const ready = email.includes('@') && password.trim() !== '' && (kind === 'gmail' || host.trim() !== '');
  const submit = async () => {
    if (!ready || busy) return;
    setBusy(true);
    setError(null);
    try {
      const account = await useMail.getState().add({
        kind,
        email: email.trim(),
        password,
        host: kind === 'imap' ? host.trim() : null,
        port: kind === 'imap' ? Number(port) || null : null,
        username: kind === 'imap' && username.trim() ? username.trim() : null,
        // Signing in again keeps the name the account has.
        nickname: initial ? null : nickname.trim() || null,
      });
      onDone(account);
    } catch (e) {
      setError(errorText(e));
    } finally {
      setBusy(false);
    }
  };
  const submitOnEnter = (e: KeyboardEvent) => e.key === 'Enter' && void submit();

  return (
    <div className={styles.connect}>
      {!initial && <SegmentedControl value={kind} onChange={setKind} options={KINDS} />}
      <Field label="Email address" htmlFor={`${id}-email`}>
        <TextField
          id={`${id}-email`}
          icon={AtSign}
          type="email"
          placeholder={kind === 'gmail' ? 'you@gmail.com' : 'you@example.com'}
          value={email}
          onChange={(e) => setEmail(e.target.value)}
          onKeyDown={submitOnEnter}
          autoFocus={!initial}
          spellCheck={false}
        />
      </Field>
      <Field
        label={kind === 'gmail' ? 'App password' : 'Password'}
        htmlFor={`${id}-password`}
        description={
          kind === 'gmail' ? (
            <>
              Gmail signs in other apps with an app password: turn on 2-Step Verification in your Google Account, then
              create one and paste its 16 letters here.{' '}
              <a
                href={APP_PASSWORDS_URL}
                onClick={(e) => {
                  e.preventDefault();
                  void openUrl(APP_PASSWORDS_URL);
                }}
              >
                Create an app password <ExternalLink size={11} />
              </a>
            </>
          ) : (
            'Some providers ask for an app password here too.'
          )
        }
      >
        <TextField
          id={`${id}-password`}
          icon={KeyRound}
          type="password"
          placeholder={kind === 'gmail' ? 'abcd efgh ijkl mnop' : undefined}
          value={password}
          invalid={!!error}
          onChange={(e) => setPassword(e.target.value)}
          onKeyDown={submitOnEnter}
          autoFocus={!!initial}
        />
      </Field>
      {kind === 'imap' && (
        <>
          <div className={styles.connectRow}>
            <Field label="IMAP server" htmlFor={`${id}-host`} className={styles.grow}>
              <TextField
                id={`${id}-host`}
                icon={Server}
                placeholder="imap.example.com"
                value={host}
                onChange={(e) => setHost(e.target.value)}
                onKeyDown={submitOnEnter}
                spellCheck={false}
              />
            </Field>
            <Field label="Port" htmlFor={`${id}-port`} className={styles.port}>
              <TextField
                id={`${id}-port`}
                inputMode="numeric"
                value={port}
                onChange={(e) => setPort(e.target.value.replace(/\D/g, '').slice(0, 5))}
                onKeyDown={submitOnEnter}
              />
            </Field>
          </div>
          <Field label="Username" htmlFor={`${id}-user`} description="Leave empty to sign in with the email address.">
            <TextField
              id={`${id}-user`}
              icon={User}
              value={username}
              onChange={(e) => setUsername(e.target.value)}
              onKeyDown={submitOnEnter}
              spellCheck={false}
            />
          </Field>
        </>
      )}
      {!initial && (
        <Field
          label="Name (optional)"
          htmlFor={`${id}-name`}
          description="Like Work or Personal: ask the assistant about this account by its name."
        >
          <TextField
            id={`${id}-name`}
            icon={Tag}
            placeholder="Work"
            value={nickname}
            maxLength={MAX_NAME}
            onChange={(e) => setNickname(e.target.value)}
            onKeyDown={submitOnEnter}
          />
        </Field>
      )}
      {error && <Notice tone="danger">{error}</Notice>}
      <p className={styles.connectNote}>
        Demido only reads your mail: opening a message does not mark it as read. The password is kept in your system's
        credential store.
      </p>
      <div className={styles.connectActions}>
        {onCancel && (
          <Button variant="ghost" onClick={onCancel} disabled={busy}>
            Cancel
          </Button>
        )}
        <Button variant="primary" loading={busy} disabled={!ready} onClick={() => void submit()}>
          {initial ? 'Sign in' : 'Connect'}
        </Button>
      </div>
    </div>
  );
}
