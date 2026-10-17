import { type MouseEvent, type ReactNode, useEffect, useState } from 'react';
import {
  BadgeCheck,
  Check,
  Download,
  ExternalLink,
  Heart,
  KeyRound,
  Lock,
  Pause,
  Play,
  RefreshCw,
  RotateCcw,
  Star,
} from 'lucide-react';
import {
  Button,
  IconButton,
  Notice,
  SegmentedControl,
  Spinner,
  Switch,
  TextField,
  Tooltip,
  cx,
  formatBytes,
} from '@demido/ui';

import { api, errorText } from '@/lib/api';
import type { DownloadJob, HfModelFile, HfRepo, HfRepoFiles, MemoryBudget, ModelEntry } from '@/lib/types';
import { CAPABILITIES } from '@/settings/models/Capabilities';
import { useChats } from '@/stores/chats';
import { useModels } from '@/stores/models';
import { toast } from '@/stores/toasts';
import { useDownloadsPanel } from '@/downloads/panel';
import { isRunning, queuePosition, shortName } from '@/downloads/queue';
import { localModelFor, useLocalModel } from './actions';
import { bestFile, fitFor } from './fit';
import { formatAgo, formatContext, formatLicense, formatParams } from './format';
import { MemoryBar } from './MemoryBar';
import { ModelCard } from './ModelCard';
import { isKnownQuantizer } from './ModelsWindow';
import { QuantPicker } from './QuantPicker';
import styles from './ModelsWindow.module.css';

/** What became of a file here: the model it is in the library, or its download. */
interface FileStatus {
  model?: ModelEntry;
  job?: DownloadJob;
}

function statusOf(repo: string, file: HfModelFile, models: ModelEntry[], downloads: DownloadJob[]): FileStatus {
  const model = localModelFor(models, file.paths[0] ?? file.name);
  if (model) return { model };
  return { job: downloads.find((j) => j.repo === repo && j.name === file.name && j.state !== 'done') };
}

/**
 * One repository: what to download first (the quantization, how it fits, the button), then what
 * the repository says about the model.
 */
export function RepoDetail({ repo, budget }: { repo: HfRepo; budget: MemoryBudget }) {
  const models = useModels((st) => st.models);
  const downloads = useModels((st) => st.downloads);
  const [data, setData] = useState<HfRepoFiles | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [attempt, setAttempt] = useState(0);
  const [chosenName, setChosenName] = useState<string | null>(null);
  const [withProjector, setWithProjector] = useState(true);
  const [hasToken, setHasToken] = useState(false);

  useEffect(() => {
    let current = true;
    setError(null);
    api.hfRepoFiles(repo.id).then(
      (d) => current && setData(d),
      (e) => current && setError(errorText(e)),
    );
    void api.hasHfToken().then((t) => current && setHasToken(t));
    return () => {
      current = false;
    };
  }, [repo.id, attempt]);

  // The repository's own page is fresher than the search row, and fills in a row the search did not return.
  const info = data?.info ?? repo;
  const projector = data?.projector && withProjector ? data.projector.size : 0;
  const best = data ? bestFile(data.files, info.parameters, projector, budget) : undefined;
  const chosen = data?.files.find((f) => f.name === chosenName) ?? best;
  const fit = chosen ? fitFor(chosen.size + projector, budget) : null;
  const blocked = info.gated && !hasToken;

  const status = (file: HfModelFile) => statusOf(repo.id, file, models, downloads);
  const download = async (file: HfModelFile) => {
    try {
      const job = await api.downloadModel(repo.id, file, withProjector ? (data?.projector ?? null) : null);
      useModels.getState().upsertDownload(job);
      const ahead = useModels
        .getState()
        .downloads.filter((j) => j.id !== job.id && (isRunning(j) || j.state === 'queued'));
      toast.info(
        ahead.length ? 'Added to the queue' : 'Download started',
        `${shortName(repo.id)} ${file.quant ?? ''} · ${formatBytes(job.total)}`,
      );
    } catch (e) {
      toast.error('Could not start the download', errorText(e));
    }
  };

  return (
    <div className={styles.detailScroll}>
      <header className={styles.detailHead}>
        <div className={styles.grow}>
          <h2 className={styles.title}>{shortName(repo.id)}</h2>
          <div className={styles.byline}>
            <span>{info.author}</span>
            {isKnownQuantizer(info.author) && (
              <Tooltip content="A well-known quantizer" delay={200}>
                <BadgeCheck size={13} className={styles.verified} aria-label="A well-known quantizer" />
              </Tooltip>
            )}
            {info.downloads > 0 && (
              <>
                <span className={styles.dot}>·</span>
                <span className={styles.stat}>
                  <Download size={12} aria-hidden />
                  {info.downloads.toLocaleString('en')}
                </span>
                <span className={styles.stat}>
                  <Heart size={12} aria-hidden />
                  {info.likes.toLocaleString('en')}
                </span>
              </>
            )}
            {formatAgo(info.lastModified) && (
              <>
                <span className={styles.dot}>·</span>
                <span className={styles.stat}>Updated {formatAgo(info.lastModified)}</span>
              </>
            )}
          </div>
        </div>
        <IconButton
          icon={ExternalLink}
          label="Open on Hugging Face"
          onClick={() => void api.openUrl(`https://huggingface.co/${repo.id}`)}
        />
      </header>

      {info.gated && <GatedNotice hasToken={hasToken} onSaved={() => setHasToken(true)} />}

      <section className={styles.card} aria-label="Download">
        <div className={styles.cardHead}>
          <h3 className={styles.heading}>Download</h3>
          <span className={styles.grow} />
          {chosen && best && chosen.name !== best.name && (
            <Button size="sm" variant="ghost" icon={Star} onClick={() => setChosenName(best.name)}>
              Back to the best fit ({best.quant ?? best.name})
            </Button>
          )}
        </div>
        {error ? (
          <Notice
            tone="danger"
            title="The files could not be listed"
            action={
              <Button size="sm" variant="secondary" icon={RefreshCw} onClick={() => setAttempt((a) => a + 1)}>
                Retry
              </Button>
            }
          >
            {error}
          </Notice>
        ) : !data ? (
          <div className={styles.cardLoading}>
            <Spinner size={14} /> Reading the repository’s files
          </div>
        ) : !chosen || !fit ? (
          <p className={styles.muted}>This repository has no GGUF files Demido can run.</p>
        ) : (
          <>
            <div className={styles.downloadRow}>
              <QuantPicker
                files={data.files}
                parameters={info.parameters}
                chosen={chosen}
                best={best}
                projector={projector}
                budget={budget}
                onChoose={(f) => setChosenName(f.name)}
                status={(f) => <FileBadge status={status(f)} />}
              />
              <PrimaryAction
                status={status(chosen)}
                bytes={chosen.size + projector}
                blocked={blocked}
                tooLarge={fit.key === 'large'}
                onDownload={() => void download(chosen)}
              />
            </div>
            {data.projector && (
              <label className={styles.projector}>
                <Switch checked={withProjector} onChange={setWithProjector} size="sm" />
                <span>
                  Include the {info.capabilities.audio ? 'audio and vision' : 'vision'} projector
                  <span className={styles.projectorNote}>
                    {data.projector.path.split('/').pop()}, {formatBytes(data.projector.size)}. It lets the model{' '}
                    {info.capabilities.audio ? 'hear sound and see pictures' : 'see pictures'}, and one serves every
                    quant here.
                  </span>
                </span>
              </label>
            )}
            <MemoryBar bytes={chosen.size + projector} fit={fit} budget={budget} />
            <p className={styles.verdict}>
              <span className={cx(styles.pill, styles[`tone-${fit.tone}`])}>{fit.label}</span>
              <span>
                <b>{chosen.quant ?? chosen.name}</b>
                {projector > 0 && ' with its projector'}. {fit.text}
              </span>
            </p>
          </>
        )}
      </section>

      <dl className={styles.facts}>
        <Fact name="Parameters" value={formatParams(info.parameters)} />
        <Fact name="Architecture" value={info.architecture} mono />
        <Fact name="Context" value={formatContext(info.contextLength)} />
        <Fact name="License" value={formatLicense(info.license)} />
        <Fact
          name="Format"
          value={data ? `GGUF, ${data.files.length} ${data.files.length === 1 ? 'quant' : 'quants'}` : 'GGUF'}
        />
        {info.baseModel && (
          <div className={styles.fact}>
            <dt>Based on</dt>
            <dd>
              <button
                type="button"
                className={styles.link}
                onClick={() => void api.openUrl(`https://huggingface.co/${info.baseModel}`)}
              >
                {info.baseModel}
              </button>
            </dd>
          </div>
        )}
      </dl>

      <section className={styles.section} aria-label="Capabilities">
        <h3 className={styles.heading}>Capabilities, as the publisher states them</h3>
        <div className={styles.capabilities}>
          {CAPABILITIES.map(({ key, icon: Icon, label, meaning }) => {
            const value = info.capabilities[key];
            return (
              <Tooltip
                key={key}
                content={
                  value === null ? `${meaning}: not stated` : value ? meaning : `Does not: ${meaning.toLowerCase()}`
                }
                delay={200}
              >
                <span className={styles.capability} data-state={value === null ? 'unknown' : value ? 'yes' : 'no'}>
                  <Icon size={14} strokeWidth={1.8} aria-hidden />
                  {label}
                  {value === null && <span className={styles.unknown}>?</span>}
                </span>
              </Tooltip>
            );
          })}
        </div>
      </section>

      <CardSection repo={repo.id} base={info.baseModel} />
    </div>
  );
}

/** Whose card is shown: the repository's own, or the one of the model it was made from. */
type CardSource = 'repo' | 'base';

/**
 * The model card, whole. A quantizer's card is mostly about its files, so when the repository
 * names the model it was made from, that card is a switch away. Each is fetched the first time it
 * is shown, after the download options, which never wait for it.
 */
function CardSection({ repo, base }: { repo: string; base: string | null }) {
  const [source, setSource] = useState<CardSource>('repo');
  const [cards, setCards] = useState<Record<string, string | null | undefined>>({});
  const [failed, setFailed] = useState<Record<string, boolean>>({});
  const id = source === 'base' && base ? base : repo;
  const markdown = cards[id];

  useEffect(() => {
    if (id in cards) return;
    let current = true;
    api.hfModelCard(id).then(
      (card) => current && setCards((c) => ({ ...c, [id]: card })),
      () => current && setFailed((f) => ({ ...f, [id]: true })),
    );
    return () => {
      current = false;
    };
  }, [id, cards]);

  const hasBase = base !== null && base.toLowerCase() !== repo.toLowerCase();
  return (
    <section className={styles.section} aria-label="Model card">
      <div className={styles.cardSectionHead}>
        <h3 className={styles.heading}>Model card</h3>
        <span className={styles.grow} />
        {hasBase && (
          <SegmentedControl
            size="sm"
            value={source}
            onChange={setSource}
            options={[
              { value: 'repo', label: 'This repository' },
              { value: 'base', label: `Base: ${shortName(base)}` },
            ]}
          />
        )}
        <IconButton
          icon={ExternalLink}
          label="Open the card on Hugging Face"
          size="sm"
          onClick={() => void api.openUrl(`https://huggingface.co/${id}`)}
        />
      </div>
      {failed[id] && markdown === undefined ? (
        <p className={styles.muted}>The model card could not be read.</p>
      ) : markdown === undefined ? (
        <div className={styles.cardLoading}>
          <Spinner size={14} /> Reading the model card
        </div>
      ) : markdown === null ? (
        <p className={styles.muted}>{id} has no model card.</p>
      ) : (
        <ModelCard key={id} markdown={markdown} repo={id} />
      )}
    </section>
  );
}

function Fact({ name, value, mono }: { name: string; value: string | null; mono?: boolean }) {
  return (
    <div className={styles.fact}>
      <dt>{name}</dt>
      <dd className={cx(mono && styles.mono, value === null && styles.unstated)}>{value ?? 'Not stated'}</dd>
    </div>
  );
}

/** The big button: what can be done with the chosen file now. */
function PrimaryAction({
  status,
  bytes,
  blocked,
  tooLarge,
  onDownload,
}: {
  status: FileStatus;
  bytes: number;
  blocked: boolean;
  tooLarge: boolean;
  onDownload: () => void;
}) {
  const openPanel = useDownloadsPanel((st) => st.setOpen);
  const picked = useChats((st) => st.pickedModelId);
  const downloads = useModels((st) => st.downloads);
  const { model, job } = status;

  if (model) {
    const inUse = picked === model.id || (picked === null && model.isDefault);
    return inUse ? (
      <Button variant="primary" size="lg" icon={Check} disabled>
        In use
      </Button>
    ) : (
      <Button variant="primary" size="lg" icon={Play} onClick={() => void useLocalModel(model)}>
        Use this model
      </Button>
    );
  }
  if (job) {
    if (job.state === 'paused') {
      return (
        <Button variant="secondary" size="lg" icon={Play} onClick={() => void api.resumeDownload(job.id)}>
          Resume, {Math.floor((job.downloaded / Math.max(1, job.total)) * 100)}%
        </Button>
      );
    }
    if (job.state === 'failed') {
      return (
        <Button variant="secondary" size="lg" icon={RotateCcw} onClick={() => void api.resumeDownload(job.id)}>
          Retry
        </Button>
      );
    }
    const label =
      job.state === 'queued'
        ? `Queued, #${queuePosition(downloads, job.id) ?? 1}`
        : job.state === 'verifying'
          ? 'Verifying'
          : `Downloading ${Math.floor((job.downloaded / Math.max(1, job.total)) * 100)}%`;
    return (
      <Button variant="secondary" size="lg" icon={Download} onClick={() => openPanel(true)}>
        {label}
      </Button>
    );
  }
  if (blocked) {
    return (
      <Button variant="primary" size="lg" icon={Lock} disabled>
        Needs a Hugging Face token
      </Button>
    );
  }
  return (
    <Tooltip content="Larger than this computer can load" disabled={!tooLarge} delay={200}>
      <Button variant="primary" size="lg" icon={Download} disabled={tooLarge} onClick={onDownload}>
        Download {formatBytes(bytes)}
      </Button>
    </Tooltip>
  );
}

/** A file's state at the end of its row in the quant dropdown, with the one action that fits it. */
function FileBadge({ status }: { status: FileStatus }): ReactNode {
  const picked = useChats((st) => st.pickedModelId);
  const downloads = useModels((st) => st.downloads);
  const { model, job } = status;
  const stop = (e: MouseEvent) => e.stopPropagation();
  if (model) {
    const inUse = picked === model.id || (picked === null && model.isDefault);
    return (
      <span className={cx(styles.pill, styles['tone-accent'])}>
        <Check size={10} strokeWidth={2.6} aria-hidden />
        {inUse ? 'In use' : 'Installed'}
      </span>
    );
  }
  if (!job) return null;
  const pct = Math.floor((job.downloaded / Math.max(1, job.total)) * 100);
  switch (job.state) {
    case 'downloading':
      return (
        <>
          <span className={styles.mono}>{pct}%</span>
          <span className={styles.mini}>
            <span style={{ width: `${pct}%` }} />
          </span>
          <IconButton
            icon={Pause}
            label="Pause"
            size="xs"
            onClick={(e) => {
              stop(e);
              void api.pauseDownload(job.id);
            }}
          />
        </>
      );
    case 'verifying':
      return <span className={cx(styles.pill, styles['tone-info'])}>Verifying</span>;
    case 'queued':
      return <span className={styles.pill}>Queued #{queuePosition(downloads, job.id) ?? 1}</span>;
    case 'paused':
      return (
        <>
          <span className={styles.mono}>{pct}%</span>
          <span className={cx(styles.mini, styles.miniPaused)}>
            <span style={{ width: `${pct}%` }} />
          </span>
          <IconButton
            icon={Play}
            label="Resume"
            size="xs"
            onClick={(e) => {
              stop(e);
              void api.resumeDownload(job.id);
            }}
          />
        </>
      );
    case 'failed':
      return (
        <>
          <span className={cx(styles.pill, styles['tone-danger'])}>Failed</span>
          <IconButton
            icon={RotateCcw}
            label="Retry"
            size="xs"
            onClick={(e) => {
              stop(e);
              void api.resumeDownload(job.id);
            }}
          />
        </>
      );
    default:
      return null;
  }
}

/** A gated repository: what it needs, and where to give it. */
function GatedNotice({ hasToken, onSaved }: { hasToken: boolean; onSaved: () => void }) {
  const [value, setValue] = useState('');
  const [saving, setSaving] = useState(false);
  if (hasToken) {
    return (
      <Notice tone="info" icon={KeyRound} title="Gated, and your token is used">
        The publisher asks for an account that accepted their terms. Downloads use the Hugging Face token you saved; if
        one fails as refused, accept the terms on the model’s page first.
      </Notice>
    );
  }
  const save = async () => {
    setSaving(true);
    try {
      if (await api.setHfToken(value)) {
        setValue('');
        onSaved();
        toast.success('Token saved', 'It is kept in your system’s credential manager.');
      } else {
        toast.error('The token could not be saved');
      }
    } finally {
      setSaving(false);
    }
  };
  return (
    <Notice tone="warning" icon={Lock} title="Gated">
      <p className={styles.noticeText}>
        The publisher asks for a Hugging Face account that accepted their terms. Accept them on the model’s page, then
        paste an access token here. unsloth and bartowski publish ungated copies of most models.
      </p>
      <div className={styles.tokenRow}>
        <TextField
          icon={KeyRound}
          type="password"
          placeholder="hf_…"
          aria-label="Hugging Face access token"
          value={value}
          onChange={(e) => setValue(e.target.value)}
          onKeyDown={(e) => e.key === 'Enter' && value.trim() && void save()}
        />
        <Button variant="secondary" disabled={!value.trim()} loading={saving} onClick={() => void save()}>
          Save token
        </Button>
      </div>
    </Notice>
  );
}
