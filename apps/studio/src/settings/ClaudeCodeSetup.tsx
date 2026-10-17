import { useCallback, useEffect, useId, useState } from 'react';
import { Check, Copy, ExternalLink, KeyRound, LogIn, LogOut, RefreshCw, Tag } from 'lucide-react';
import { Badge, Button, Field, Notice, Spinner, TextField } from '@demido/ui';

import { api, errorText } from '@/lib/api';
import type { ClaudeCodeAccount, ClaudeCodeLoginState, ClaudeCodeStatus } from '@/lib/types';
import { toast } from '@/stores/toasts';
import s from './settings.module.css';
import styles from './ProvidersTab.module.css';

const NAME_MAX = 60;

/** "Max plan", "Pro plan", or the Console account a sign-in uses. */
export function planLabel(account: ClaudeCodeAccount): string {
  if (account.authMethod === 'console') return 'Claude Console account';
  const plan = account.subscriptionType;
  return plan ? `${plan.charAt(0).toUpperCase()}${plan.slice(1)} plan` : 'Claude account';
}

/** A Claude Code profile's status (without one, the next free profile's), read once and again on demand. */
export function useClaudeCodeStatus(profile?: string) {
  const [status, setStatus] = useState<ClaudeCodeStatus | null>(null);
  const [checking, setChecking] = useState(false);
  const check = useCallback(async () => {
    setChecking(true);
    try {
      setStatus(await api.claudeCodeStatus(profile));
    } catch (e) {
      toast.error('Could not check Claude Code', errorText(e));
    } finally {
      setChecking(false);
    }
  }, [profile]);
  useEffect(() => void check(), [check]);
  return { status, checking, check };
}

/** How to install Claude Code on this system, with a button to copy the command. */
function InstallSteps({
  status,
  onCheck,
  checking,
}: {
  status: ClaudeCodeStatus;
  onCheck: () => void;
  checking: boolean;
}) {
  const [copied, setCopied] = useState(false);
  const { shell, command, docs } = status.install;
  return (
    <div className={s.stack}>
      <Notice tone="warning" title="Claude Code is not installed">
        Demido uses your Claude plan through Claude Code, Anthropic’s official app. Install it by running this in{' '}
        {shell === 'PowerShell' ? 'PowerShell' : 'a terminal'}, then check again.
      </Notice>
      <div className={styles.command}>
        <code>{command}</code>
        <Button
          size="sm"
          variant="ghost"
          icon={copied ? Check : Copy}
          onClick={() =>
            void navigator.clipboard.writeText(command).then(() => {
              setCopied(true);
              setTimeout(() => setCopied(false), 1500);
            })
          }
        >
          {copied ? 'Copied' : 'Copy'}
        </Button>
      </div>
      <div className={styles.formActions}>
        <Button variant="ghost" icon={ExternalLink} onClick={() => void api.openUrl(docs)}>
          Other ways to install
        </Button>
        <Button variant="secondary" icon={RefreshCw} loading={checking} onClick={onCheck}>
          Check again
        </Button>
      </div>
    </div>
  );
}

/** Signs a profile in: the browser opens Claude's sign-in page, and a code it shows is pasted here. */
export function SignIn({ profile, onSignedIn }: { profile: string; onSignedIn: (account: ClaudeCodeAccount) => void }) {
  const id = useId();
  const [login, setLogin] = useState<ClaudeCodeLoginState>({ state: 'idle' });
  const [code, setCode] = useState('');
  const [busy, setBusy] = useState(false);

  const settle = useCallback(
    (next: ClaudeCodeLoginState) => {
      setLogin(next);
      if (next.state === 'done') onSignedIn(next.account);
    },
    [onSignedIn],
  );

  // While the browser is open, notice when signing in finished there.
  useEffect(() => {
    if (login.state !== 'waiting') return;
    const timer = setInterval(() => {
      void api.claudeCodeLoginState().then((next) => next.state !== 'waiting' && settle(next));
    }, 1500);
    return () => clearInterval(timer);
  }, [login.state, settle]);

  useEffect(() => () => void api.claudeCodeLoginCancel(), []);

  const start = async () => {
    setBusy(true);
    try {
      const next = await api.claudeCodeLogin(profile);
      settle(next);
      if (next.state === 'waiting' && next.url) void api.openUrl(next.url);
    } catch (e) {
      setLogin({ state: 'failed', message: errorText(e) });
    } finally {
      setBusy(false);
    }
  };

  const sendCode = async () => {
    setBusy(true);
    try {
      settle(await api.claudeCodeLoginCode(code.trim()));
      setCode('');
    } catch (e) {
      setLogin({ state: 'failed', message: errorText(e) });
    } finally {
      setBusy(false);
    }
  };

  if (login.state === 'waiting') {
    return (
      <div className={s.stack}>
        <Notice tone="info" icon={LogIn}>
          Sign in with your Claude account in the browser.{' '}
          {login.url && (
            <a
              href={login.url}
              onClick={(e) => {
                e.preventDefault();
                void api.openUrl(login.url!);
              }}
            >
              Open the page again <ExternalLink size={11} />
            </a>
          )}
        </Notice>
        <Field
          label="Code"
          htmlFor={`${id}-code`}
          description="If the page shows a code instead of finishing on its own, paste it here."
        >
          <TextField
            id={`${id}-code`}
            icon={KeyRound}
            value={code}
            onChange={(e) => setCode(e.target.value)}
            onKeyDown={(e) => e.key === 'Enter' && code.trim() && !busy && void sendCode()}
          />
        </Field>
        <div className={styles.formActions}>
          <Button
            variant="ghost"
            onClick={() => {
              void api.claudeCodeLoginCancel();
              setLogin({ state: 'idle' });
            }}
          >
            Cancel
          </Button>
          <Button variant="primary" loading={busy} disabled={!code.trim()} onClick={() => void sendCode()}>
            Submit code
          </Button>
        </div>
      </div>
    );
  }

  return (
    <div className={s.stack}>
      {login.state === 'failed' && <Notice tone="danger">{login.message}</Notice>}
      <div className={styles.formActions}>
        <Button variant="primary" icon={LogIn} loading={busy} onClick={() => void start()}>
          Sign in with Claude
        </Button>
      </div>
    </div>
  );
}

/**
 * Connects a Claude subscription in the API key form's layout: a name, then the code Claude's
 * sign-in page shows where the key would go. Claude Code must be installed first.
 */
export function SubscriptionForm({ onCancel, onAdded }: { onCancel?: () => void; onAdded: () => void }) {
  const id = useId();
  const { status, checking, check } = useClaudeCodeStatus();
  const [login, setLogin] = useState<ClaudeCodeLoginState>({ state: 'idle' });
  const [name, setName] = useState('');
  const [code, setCode] = useState('');
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const settle = useCallback(
    (next: ClaudeCodeLoginState) => {
      setLogin(next);
      if (next.state === 'done') void check();
      if (next.state === 'failed') setError(next.message);
    },
    [check],
  );

  // While the sign-in page is open, notice when signing in finished there.
  useEffect(() => {
    if (login.state !== 'waiting') return;
    const timer = setInterval(() => {
      void api.claudeCodeLoginState().then((next) => next.state !== 'waiting' && settle(next));
    }, 1500);
    return () => clearInterval(timer);
  }, [login.state, settle]);

  useEffect(() => () => void api.claudeCodeLoginCancel(), []);

  const profile = status?.profile;
  const account = login.state === 'done' ? login.account : status?.account;
  const signedIn = !!account?.loggedIn;

  /** Opens Claude's sign-in page, the one "Sign in with Claude" opened, which shows the code. */
  const getCode = async () => {
    if (!profile) return;
    setError(null);
    if (login.state === 'waiting' && login.url) return void api.openUrl(login.url);
    try {
      const next = await api.claudeCodeLogin(profile);
      settle(next);
      if (next.state === 'waiting' && next.url) void api.openUrl(next.url);
    } catch (e) {
      setError(errorText(e));
    }
  };

  const switchAccount = async () => {
    if (!profile) return;
    setBusy(true);
    setError(null);
    try {
      await api.claudeCodeLogout(profile);
      setLogin({ state: 'idle' });
      await check();
    } catch (e) {
      setError(errorText(e));
    } finally {
      setBusy(false);
    }
  };

  const connect = async () => {
    if (!profile) return;
    setBusy(true);
    setError(null);
    try {
      if (!signedIn) {
        const next = await api.claudeCodeLoginCode(code.trim());
        setCode('');
        settle(next);
        if (next.state === 'failed') return;
        if (next.state !== 'done') throw new Error('Claude has not finished signing you in. Try again in a moment.');
      }
      const added = await api.addProvider('claudecode', '', name.trim() || undefined, undefined, profile);
      toast.success('Anthropic connected', `${added.models.length} models are available with your Claude plan.`);
      onAdded();
    } catch (e) {
      setError(errorText(e));
    } finally {
      setBusy(false);
    }
  };
  const ready = !!status?.installed && (signedIn || !!code.trim());

  return (
    <div className={s.stack}>
      <Field label="Name" htmlFor={`${id}-name`} description="Shown in the model list. Optional.">
        <TextField
          id={`${id}-name`}
          icon={Tag}
          placeholder="Anthropic"
          value={name}
          maxLength={NAME_MAX}
          onChange={(e) => setName(e.target.value)}
          onKeyDown={(e) => e.key === 'Enter' && ready && !busy && void connect()}
        />
      </Field>
      {!status ? (
        <Spinner size={18} />
      ) : !status.installed ? (
        <InstallSteps status={status} onCheck={() => void check()} checking={checking} />
      ) : signedIn && account ? (
        <Field label="Account" description="Its plan’s limits apply to every chat with these models.">
          <div className={styles.accountRow}>
            <Badge tone="accent">Signed in</Badge>
            <span>
              {account.email ?? 'Claude account'} · {planLabel(account)}
            </span>
            <span className={styles.flex} />
            <Button size="sm" variant="ghost" icon={LogOut} disabled={busy} onClick={() => void switchAccount()}>
              Use another account
            </Button>
          </div>
        </Field>
      ) : (
        <Field
          label="Sign-in code"
          htmlFor={`${id}-code`}
          description={
            <>
              Sign in with your Claude Pro or Max account and paste the code the page shows.{' '}
              <a
                href={login.state === 'waiting' && login.url ? login.url : '#'}
                onClick={(e) => {
                  e.preventDefault();
                  void getCode();
                }}
              >
                Get a code <ExternalLink size={11} />
              </a>
            </>
          }
        >
          <TextField
            id={`${id}-code`}
            icon={KeyRound}
            type="password"
            placeholder={login.state === 'waiting' ? 'Paste the code' : 'Get a code first'}
            value={code}
            invalid={!!error}
            onChange={(e) => setCode(e.target.value)}
            onKeyDown={(e) => e.key === 'Enter' && ready && !busy && void connect()}
          />
        </Field>
      )}
      {error && <Notice tone="danger">{error}</Notice>}
      <div className={styles.formActions}>
        {onCancel && (
          <Button variant="ghost" onClick={onCancel} disabled={busy}>
            Cancel
          </Button>
        )}
        <Button variant="primary" loading={busy} disabled={!ready} onClick={() => void connect()}>
          Connect
        </Button>
      </div>
    </div>
  );
}

/** A connected Claude subscription: who is signed in with its profile, and signing in or out. */
export function ClaudeCodeAccountRow({ profile, onChange }: { profile: string; onChange: () => void }) {
  const { status, checking, check } = useClaudeCodeStatus(profile);
  const [busy, setBusy] = useState(false);
  if (!status) return null;
  if (!status.installed) {
    return (
      <div className={s.cardPad}>
        <InstallSteps status={status} onCheck={() => void check()} checking={checking} />
      </div>
    );
  }
  const account = status.account;
  if (!account.loggedIn) {
    return (
      <div className={s.cardPad}>
        <SignIn
          profile={profile}
          onSignedIn={() => {
            void check();
            onChange();
          }}
        />
      </div>
    );
  }
  return (
    <div className={styles.providerActions}>
      <span className={styles.accountRow}>
        {account.email ?? 'Claude account'} · {planLabel(account)}
      </span>
      <span className={styles.flex} />
      <Button size="sm" variant="secondary" icon={RefreshCw} loading={checking} onClick={() => void check()}>
        Check
      </Button>
      <Button
        size="sm"
        variant="ghost"
        icon={LogOut}
        loading={busy}
        onClick={() => {
          setBusy(true);
          api
            .claudeCodeLogout(profile)
            .then(
              () => check(),
              (e) => toast.error('Could not sign out', errorText(e)),
            )
            .finally(() => setBusy(false));
        }}
      >
        Sign out
      </Button>
    </div>
  );
}
