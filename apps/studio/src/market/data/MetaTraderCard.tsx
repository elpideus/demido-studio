// The Data tab's MetaTrader card, shown once a MetaTrader 5 terminal is found on this computer. Its
// dialog sets MetaTrader up as the downloads' second source: the notice first, then the terminal,
// the account and which source downloads first. The card says what MetaTrader is doing.

import { useCallback, useEffect, useMemo, useState } from 'react';
import { MonitorCog, PlugZap, RefreshCw, TriangleAlert } from 'lucide-react';
import {
  Button,
  Checkbox,
  Dialog,
  Field,
  IconButton,
  Notice,
  ProgressBar,
  SegmentedControl,
  Select,
  Spinner,
  Switch,
} from '@demido/ui';

import { api, errorText } from '@/lib/api';
import { on } from '@/lib/events';
import type { MetaTraderDetection, MetaTraderSetup, MetaTraderStatus, Settings } from '@/lib/types';
import { useApp } from '@/stores/app';
import { toast } from '@/stores/toasts';
import {
  ORDER_OPTIONS,
  ORDER_TEXT,
  accountKey,
  accountLabel,
  chosenAccount,
  chosenTerminal,
  consented,
  depthText,
  orderFields,
  setupText,
  sourceOrder,
  stateText,
  type SourceOrder,
} from './metatrader';
import styles from './MetaTrader.module.css';

interface MetaTraderState {
  /** Null until the first look. */
  detection: MetaTraderDetection | null;
  /** Null while the market service is not running. */
  status: MetaTraderStatus | null;
  setup: MetaTraderSetup | null;
  detect: () => Promise<void>;
  setStatus: (status: MetaTraderStatus) => void;
}

function useMetaTrader(): MetaTraderState {
  const [detection, setDetection] = useState<MetaTraderDetection | null>(null);
  const [status, setStatus] = useState<MetaTraderStatus | null>(null);
  const [setup, setSetup] = useState<MetaTraderSetup | null>(null);

  const detect = useCallback(async () => {
    try {
      const found = await api.marketMetaTraderDetect();
      setDetection(found);
      setSetup(found.setup);
    } catch {
      // Not knowing about a terminal only hides the card.
    }
  }, []);

  useEffect(() => {
    void detect();
    void api
      .marketMetaTraderStatus()
      .then(setStatus)
      .catch(() => undefined);
    const unlisten = on('market://event', (e) => {
      if (e.event === 'metatrader.status') setStatus(e.params);
      else if (e.event === 'metatrader.setup') setSetup(e.params);
    });
    return () => void unlisten.then((f) => f());
  }, [detect]);

  return { detection, status, setup, detect, setStatus };
}

export function MetaTraderCard() {
  const mt = useMetaTrader();
  const settings = useApp((s) => s.settings);
  const [open, setOpen] = useState(false);
  const detection = mt.detection;
  if (!detection || (detection.terminals.length === 0 && !settings?.metatraderTerminal)) return null;

  const agreed = consented(settings, detection.consentVersion);
  const terminal = chosenTerminal(detection, settings);
  const account = agreed ? chosenAccount(terminal, settings) : null;
  const installing = setupText(mt.setup);
  const order = settings ? sourceOrder(settings) : 'fallback';
  const days = mt.status?.days ?? 0;

  let line: string;
  if (!detection.supported) line = detection.reason ?? 'MetaTrader cannot be used on this computer.';
  else if (!agreed) line = 'Found on this computer. It can download history while Dukascopy is rate limited or down.';
  else if (!terminal) line = 'The terminal chosen is not on this computer any more.';
  else if (installing) line = installing;
  else {
    const parts = [ORDER_OPTIONS.find((o) => o.value === order)!.label, stateText(mt.status)];
    if (days > 0) parts.push(`${days.toLocaleString()} day${days === 1 ? '' : 's'} downloaded since Demido started`);
    line = parts.join(' · ');
  }

  return (
    <div className={styles.card}>
      <MonitorCog size={18} strokeWidth={1.8} className={styles.cardIcon} aria-hidden />
      <div className={styles.cardMain}>
        <div className={styles.cardTitle}>
          {terminal?.name ?? 'MetaTrader 5'}
          {account && (
            <span className={styles.cardAccount}>
              {account.login} · {account.server}
            </span>
          )}
        </div>
        <div className={mt.setup?.error && agreed ? styles.cardError : styles.cardLine}>{line}</div>
        {mt.setup?.installing && <ProgressBar size="xs" value={null} label="Installing the MetaTrader bridge" />}
      </div>
      {detection.supported && (
        <Button size="sm" variant="secondary" onClick={() => setOpen(true)}>
          {agreed ? 'Settings' : 'Set up'}
        </Button>
      )}
      {open && (
        <MetaTraderDialog
          detection={detection}
          settings={settings}
          status={mt.status}
          setup={mt.setup}
          onDetect={mt.detect}
          onStatus={mt.setStatus}
          onClose={() => setOpen(false)}
        />
      )}
    </div>
  );
}

interface DialogProps {
  detection: MetaTraderDetection;
  settings: Settings | null;
  status: MetaTraderStatus | null;
  setup: MetaTraderSetup | null;
  onDetect: () => Promise<void>;
  onStatus: (status: MetaTraderStatus) => void;
  onClose: () => void;
}

function MetaTraderDialog({ detection, settings, status, setup, onDetect, onStatus, onClose }: DialogProps) {
  const agreed = consented(settings, detection.consentVersion);
  const [step, setStep] = useState<'notice' | 'setup'>(agreed ? 'setup' : 'notice');
  const [understood, setUnderstood] = useState(false);
  const [terminalPath, setTerminalPath] = useState(() => chosenTerminal(detection, settings)?.path ?? '');
  const terminal = detection.terminals.find((t) => t.path === terminalPath) ?? null;
  const [accountId, setAccountId] = useState(() => {
    const account = chosenAccount(chosenTerminal(detection, settings), settings);
    return account ? accountKey(account) : '';
  });
  const account = terminal?.accounts.find((a) => accountKey(a) === accountId) ?? null;
  const [order, setOrder] = useState<SourceOrder>(() => (settings ? sourceOrder(settings) : 'fallback'));
  const [switchAccount, setSwitchAccount] = useState(settings?.metatraderSwitch ?? true);
  const [busy, setBusy] = useState<'save' | 'test' | 'stop' | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [tested, setTested] = useState<MetaTraderStatus | null>(null);

  // A fresh look when the dialog opens: an account logged into since shows up.
  useEffect(() => {
    void onDetect();
  }, [onDetect]);

  // The account list follows the terminal; one that went away picks the account it was last on.
  useEffect(() => {
    if (terminal && !terminal.accounts.some((a) => accountKey(a) === accountId)) {
      const next = chosenAccount(terminal, settings);
      setAccountId(next ? accountKey(next) : '');
    }
  }, [terminal, accountId, settings]);

  const dirty = useMemo(() => {
    if (!settings || !agreed) return true;
    const fields = orderFields(order);
    return (
      settings.metatraderTerminal !== terminalPath ||
      settings.metatraderLogin !== account?.login ||
      settings.metatraderServer !== account?.server ||
      settings.marketSource !== fields.source ||
      (fields.source === 'dukascopy' && settings.metatraderFallback !== fields.fallback) ||
      (fields.source === 'metatrader' && settings.dukascopyFallback !== fields.dukascopyFallback) ||
      settings.metatraderSwitch !== switchAccount
    );
  }, [settings, agreed, order, terminalPath, account, switchAccount]);

  const save = async (): Promise<boolean> => {
    if (!terminal || !account) return false;
    setError(null);
    try {
      const saved = await api.marketMetaTraderChoose({
        consent: true,
        terminal: terminal.path,
        login: account.login,
        server: account.server,
        ...orderFields(order),
        switchAccount,
      });
      useApp.setState({ settings: saved });
      return true;
    } catch (e) {
      setError(errorText(e));
      return false;
    }
  };

  const saveAndClose = async () => {
    setBusy('save');
    const ok = await save();
    setBusy(null);
    if (ok) {
      toast.success('MetaTrader is set up', ORDER_TEXT[order]);
      onClose();
    }
  };

  // Tests what is saved, so unsaved changes are saved first.
  const test = async () => {
    setBusy('test');
    setTested(null);
    try {
      if (dirty && !(await save())) return;
      const result = await api.marketMetaTraderConnect();
      setTested(result);
      onStatus(result);
    } catch (e) {
      setError(errorText(e));
    } finally {
      setBusy(null);
    }
  };

  const stop = async () => {
    setBusy('stop');
    setError(null);
    try {
      useApp.setState({ settings: await api.marketMetaTraderChoose({ consent: false }) });
      toast.info('MetaTrader is not used any more', 'Downloads come from Dukascopy only.');
      onClose();
    } catch (e) {
      setError(errorText(e));
    } finally {
      setBusy(null);
    }
  };

  if (step === 'notice') {
    return (
      <Dialog
        open
        onClose={onClose}
        width={600}
        icon={<MonitorCog size={22} strokeWidth={1.8} />}
        title="Download with MetaTrader?"
        description="Demido can download 1-minute history through your MetaTrader 5 terminal. Before it does:"
        footer={
          <>
            <Button variant="ghost" onClick={onClose}>
              Cancel
            </Button>
            <Button variant="primary" disabled={!understood} onClick={() => setStep('setup')}>
              Continue
            </Button>
          </>
        }
      >
        <ul className={styles.notice}>
          <li>
            <strong>It opens MetaTrader.</strong> When the terminal is closed, Demido starts it minimized, without
            taking focus, and leaves it running afterwards.
          </li>
          <li>
            <strong>It logs in with the password MetaTrader saved.</strong> Demido never sees it. If the terminal is on
            another account, Demido can switch to the one you choose while it downloads, and switches back afterwards.
          </li>
          <li>
            <strong>It only reads prices.</strong> It asks for candles and quotes, reads the history MetaTrader already
            keeps on disk, and never places orders or changes anything in your account.
          </li>
          <li>
            <strong>It downloads a small bridge.</strong> A Windows Python and MetaQuotes’ MetaTrader5 package (MIT
            licence), kept in Demido’s data folder.
            {detection.terminals.some((t) => t.prefix) && ' On Linux they run under Wine, in MetaTrader’s own prefix.'}
          </li>
          <li>
            <strong>Its history differs a little.</strong> Your broker’s prices can differ slightly from Dukascopy’s,
            its volume counts ticks (another unit), and it reaches back as far as the terminal has downloaded history.
            The days it downloads are marked MetaTrader in this tab.
          </li>
        </ul>
        <Checkbox
          className={styles.check}
          checked={understood}
          onChange={setUnderstood}
          label="I understand, and want Demido to use MetaTrader"
        />
      </Dialog>
    );
  }

  const installing = busy === 'save' || busy === 'test' ? setupText(setup) : null;
  const depth = depthText(terminal?.maxBars ?? null);

  return (
    <Dialog
      open
      onClose={onClose}
      width={620}
      icon={<MonitorCog size={22} strokeWidth={1.8} />}
      title="MetaTrader"
      description="Which terminal and account download, and when."
      footer={
        <>
          {agreed && (
            <Button variant="ghost" disabled={busy !== null} loading={busy === 'stop'} onClick={() => void stop()}>
              Stop using MetaTrader
            </Button>
          )}
          <span className={styles.flex} />
          <Button variant="ghost" onClick={onClose}>
            {agreed && !dirty ? 'Close' : 'Cancel'}
          </Button>
          <Button
            variant="primary"
            disabled={!account || busy !== null || (agreed && !dirty)}
            loading={busy === 'save'}
            onClick={() => void saveAndClose()}
          >
            Save
          </Button>
        </>
      }
    >
      <div className={styles.form}>
        {detection.terminals.length === 0 ? (
          <Notice tone="warning" icon={TriangleAlert} title="MetaTrader 5 is not on this computer any more">
            Install it again, or stop using it.
          </Notice>
        ) : (
          <Field
            label="Terminal"
            description={
              terminal
                ? `${terminal.prefix ? `Wine prefix ${terminal.prefix} · ` : ''}${
                    depth
                      ? `Max bars in chart ${terminal.maxBars!.toLocaleString()}: ${depth} for currency pairs, older days from its history files`
                      : terminal.windowsPath
                  }`
                : undefined
            }
          >
            {detection.terminals.length > 1 ? (
              <Select
                size="sm"
                aria-label="Terminal"
                value={terminalPath}
                onChange={(e) => setTerminalPath(e.target.value)}
                options={detection.terminals.map((t) => ({ value: t.path, label: `${t.name} · ${t.windowsPath}` }))}
              />
            ) : (
              <div className={styles.value}>{terminal?.windowsPath}</div>
            )}
          </Field>
        )}

        {terminal && (
          <Field
            label="Account"
            description="Accounts this terminal has logged into. Missing one? Log into it in MetaTrader once, with “Save password” ticked, then look again."
          >
            <div className={styles.row}>
              {terminal.accounts.length > 0 ? (
                <Select
                  size="sm"
                  aria-label="Account"
                  className={styles.grow}
                  value={accountId}
                  onChange={(e) => setAccountId(e.target.value)}
                  options={terminal.accounts.map((a) => ({ value: accountKey(a), label: accountLabel(a) }))}
                />
              ) : (
                <span className={styles.muted}>No account yet.</span>
              )}
              <IconButton icon={RefreshCw} label="Look again" size="sm" onClick={() => void onDetect()} />
            </div>
          </Field>
        )}

        <Field label="Downloads" description={ORDER_TEXT[order]}>
          <SegmentedControl size="sm" value={order} onChange={setOrder} options={ORDER_OPTIONS} />
        </Field>

        <Field
          layout="inline"
          label="Switch accounts when needed"
          description="When MetaTrader is on another account, Demido logs into this one while it downloads and back afterwards. Off, MetaTrader is used only while it is on this account."
        >
          <Switch checked={switchAccount} onChange={setSwitchAccount} label="Switch accounts when needed" />
        </Field>

        <div className={styles.test}>
          <div className={styles.testText}>
            <div className={styles.testTitle}>Connection</div>
            <div className={styles.muted}>
              {busy === 'test' && !installing
                ? 'Opening MetaTrader and logging in; this can take a minute or two.'
                : `${stateText(status)}. The test opens MetaTrader minimized if it is closed${dirty ? ', after saving' : ''}.`}
            </div>
          </div>
          <Button
            size="sm"
            variant="secondary"
            icon={PlugZap}
            disabled={!account || busy !== null}
            loading={busy === 'test'}
            onClick={() => void test()}
          >
            Test connection
          </Button>
        </div>

        {installing && (
          <div className={styles.installing}>
            <Spinner size={14} />
            <span>{installing}</span>
          </div>
        )}
        {tested && <TestResult status={tested} />}
        {!busy && setup?.error && (
          <Notice tone="danger" title="The MetaTrader bridge could not be installed">
            {setup.error} Saving again tries again.
          </Notice>
        )}
        {error && (
          <Notice tone="danger" title="That did not work">
            {error}
          </Notice>
        )}
      </div>
    </Dialog>
  );
}

function TestResult({ status }: { status: MetaTraderStatus }) {
  if (!status.account) {
    return (
      <Notice tone="warning" title="Not connected">
        {stateText(status)}
      </Notice>
    );
  }
  const a = status.account;
  const details = [
    a.name && `${a.name}${a.company ? `, ${a.company}` : ''}`,
    status.markets !== null && `${status.markets.toLocaleString()} of its symbols are Demido markets`,
    status.terminal?.connected === false && 'the terminal is not connected to its server',
  ].filter(Boolean);
  return (
    <Notice tone="accent" title={`Connected to ${a.login} on ${a.server}`}>
      {details.join(' · ')}
      {status.switched && ' MetaTrader was on another account; it goes back to it when Demido is done.'}
      {status.tied && ' Windows keeps the terminal Demido opened tied to Demido: it closes when Demido does.'}
    </Notice>
  );
}
