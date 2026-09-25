import { useEffect, useState } from 'react';
import {
  ArrowLeft,
  BadgeCheck,
  CheckCircle2,
  Download,
  Heart,
  KeyRound,
  Pause,
  Play,
  Search,
  Star,
  X,
} from 'lucide-react';
import {
  Badge,
  Button,
  EmptyState,
  Field,
  IconButton,
  Notice,
  ProgressBar,
  Spinner,
  TextField,
  formatBytes,
  formatCount,
  formatDuration,
  useDebounced,
} from '@demido/ui';

import { api, errorText } from '@/lib/api';
import type { DownloadJob, Fit, HfModelFile, HfRepo, HfRepoFiles, Recommendation } from '@/lib/types';
import { useModels } from '@/stores/models';
import { toast } from '@/stores/toasts';
import s from '../settings.module.css';
import styles from './Models.module.css';

const FIT: Record<Fit, { tone: 'accent' | 'warning' | 'danger' | 'neutral'; label: string; title: string }> = {
  fits: { tone: 'accent', label: 'Fits', title: 'Fits in your GPU memory with room for conversation' },
  tight: { tone: 'warning', label: 'Tight', title: 'Fits, but leaves little room for long conversations' },
  large: {
    tone: 'danger',
    label: 'Too large',
    title: 'Larger than your GPU memory: it will run partly on the CPU and slowly',
  },
  unknown: { tone: 'neutral', label: '', title: '' },
};

async function startDownload(repo: string, file: HfModelFile) {
  try {
    const job = await api.downloadModel(repo, file);
    useModels.getState().upsertDownload(job);
    toast.info('Download started', `${file.name} · ${formatBytes(file.size)}`);
  } catch (e) {
    toast.error('Could not start the download', errorText(e));
  }
}

function Jobs() {
  const jobs = useModels((st) => st.downloads);
  if (jobs.length === 0) return null;
  const finished = jobs.some((j) => j.state === 'done');
  return (
    <section className={s.section}>
      <h3 className={s.sectionTitle}>
        Downloads
        {finished && (
          <Button
            size="sm"
            variant="ghost"
            onClick={() => void api.clearDownloads().then(useModels.getState().setDownloads)}
          >
            Clear finished
          </Button>
        )}
      </h3>
      <div className={styles.jobs}>
        {jobs.map((job) => (
          <JobRow key={job.id} job={job} />
        ))}
      </div>
    </section>
  );
}

function JobRow({ job }: { job: DownloadJob }) {
  const fraction = job.total > 0 ? job.downloaded / job.total : null;
  const eta = job.bytesPerSecond > 0 ? (job.total - job.downloaded) / job.bytesPerSecond : null;
  return (
    <div className={styles.job}>
      <div className={styles.jobHead}>
        <div className={s.rowMain}>
          <div className={s.rowTitle}>
            {job.name}
            {job.state === 'done' && (
              <Badge tone="accent" icon={CheckCircle2}>
                Ready
              </Badge>
            )}
            {job.state === 'paused' && <Badge>Paused</Badge>}
            {job.state === 'queued' && <Badge>Waiting</Badge>}
            {job.state === 'failed' && <Badge tone="danger">Failed</Badge>}
          </div>
          <div className={s.rowMeta}>{job.repo}</div>
        </div>
        {job.state === 'downloading' && (
          <IconButton icon={Pause} label="Pause" size="sm" onClick={() => void api.pauseDownload(job.id)} />
        )}
        {(job.state === 'paused' || job.state === 'failed') && (
          <IconButton icon={Play} label="Resume" size="sm" onClick={() => void api.resumeDownload(job.id)} />
        )}
        {job.state !== 'done' && (
          <IconButton icon={X} label="Cancel and delete" size="sm" onClick={() => void api.cancelDownload(job.id)} />
        )}
      </div>
      {job.state !== 'done' && (
        <>
          <ProgressBar
            value={job.state === 'queued' ? null : fraction}
            tone={job.state === 'failed' ? 'danger' : 'accent'}
          />
          <div className={styles.jobStats}>
            <span>
              {formatBytes(job.downloaded)} of {formatBytes(job.total)}
            </span>
            <span>
              {job.state === 'downloading' && job.bytesPerSecond > 0
                ? `${formatBytes(job.bytesPerSecond)}/s · ${eta ? formatDuration(eta) : ''} left`
                : (job.error ?? '')}
            </span>
          </div>
        </>
      )}
    </div>
  );
}

function Recommended() {
  const [recs, setRecs] = useState<Recommendation[] | null>(null);
  const jobs = useModels((st) => st.downloads);
  const models = useModels((st) => st.models);
  useEffect(() => {
    void api.recommendedModels().then(setRecs);
  }, [models]);
  if (!recs || recs.length === 0) return null;
  return (
    <section className={s.section}>
      <h3 className={s.sectionTitle}>Recommended for this computer</h3>
      <div className={styles.recoGrid}>
        {recs.map((r) => {
          const downloading = jobs.some(
            (j) => j.repo === r.repo && j.name === r.file && j.state !== 'done' && j.state !== 'failed',
          );
          return (
            <div key={r.family} className={styles.reco}>
              <div className={styles.recoHead}>
                <div className={s.rowMain}>
                  <div className={s.rowTitle}>{r.name}</div>
                  <div className={s.rowMeta}>
                    {r.familyLabel} · {r.quant} · {formatBytes(r.size)}
                  </div>
                </div>
              </div>
              <p className={styles.recoText}>{r.description}</p>
              <div className={styles.recoFoot}>
                <span className={s.muted}>{r.repo}</span>
                {r.installed ? (
                  <Badge tone="accent" icon={CheckCircle2}>
                    Installed
                  </Badge>
                ) : (
                  <Button
                    size="sm"
                    variant="primary"
                    icon={Download}
                    disabled={downloading}
                    onClick={() =>
                      void startDownload(r.repo, {
                        name: r.file,
                        paths: [r.file],
                        sizes: [r.size],
                        sha256: [r.sha256],
                        size: r.size,
                        quant: r.quant,
                        fit: 'fits',
                        recommended: true,
                      })
                    }
                  >
                    {downloading ? 'Downloading' : 'Download'}
                  </Button>
                )}
              </div>
            </div>
          );
        })}
      </div>
    </section>
  );
}

function RepoFiles({ repo, onBack }: { repo: string; onBack: () => void }) {
  const [data, setData] = useState<HfRepoFiles | null>(null);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    setData(null);
    setError(null);
    api.hfRepoFiles(repo).then(setData, (e) => setError(errorText(e)));
  }, [repo]);
  return (
    <section className={s.section}>
      <button type="button" className={s.back} onClick={onBack}>
        <ArrowLeft size={14} aria-hidden /> Search results
      </button>
      <h3 className={s.sectionTitle}>{repo}</h3>
      {error && <Notice tone="danger">{error}</Notice>}
      {!data && !error && <Spinner />}
      {data?.gated && (
        <Notice tone="warning" icon={KeyRound}>
          This repository needs you to accept its terms on Hugging Face and a token with access (below).
        </Notice>
      )}
      {data && data.files.length === 0 && (
        <EmptyState compact title="No GGUF files here" description="Pick another repository." />
      )}
      {data && data.files.length > 0 && (
        <div className={s.rows}>
          {data.files.map((f) => {
            const fit = FIT[f.fit];
            return (
              <div key={f.name} className={s.row}>
                <div className={s.rowMain}>
                  <div className={s.rowTitle}>
                    {f.quant ?? f.name}
                    {f.recommended && (
                      <Badge tone="accent" icon={Star}>
                        Best fit
                      </Badge>
                    )}
                    {fit.label && (
                      <Badge tone={fit.tone} title={fit.title}>
                        {fit.label}
                      </Badge>
                    )}
                  </div>
                  <div className={s.rowMeta}>
                    {f.name}
                    {f.paths.length > 1 ? ` · ${f.paths.length} parts` : ''}
                  </div>
                </div>
                <span className={s.muted}>{formatBytes(f.size)}</span>
                <Button
                  size="sm"
                  variant={f.recommended ? 'primary' : 'secondary'}
                  icon={Download}
                  onClick={() => void startDownload(repo, f)}
                >
                  Download
                </Button>
              </div>
            );
          })}
        </div>
      )}
    </section>
  );
}

function HfToken() {
  const [has, setHas] = useState(false);
  const [value, setValue] = useState('');
  useEffect(() => {
    void api.hasHfToken().then(setHas);
  }, []);
  return (
    <section className={s.section}>
      <h3 className={s.sectionTitle}>Hugging Face token</h3>
      <div className={`${s.card} ${s.cardPad}`}>
        <Field
          label={has ? 'A token is saved' : 'Optional'}
          description="Only needed for gated or private repositories. Stored in your system's credential manager."
        >
          <div className={styles.searchRow}>
            <TextField
              icon={KeyRound}
              type="password"
              placeholder="hf_…"
              value={value}
              onChange={(e) => setValue(e.target.value)}
            />
            <Button
              variant="secondary"
              disabled={!value.trim()}
              onClick={() =>
                void api.setHfToken(value).then(() => {
                  setHas(true);
                  setValue('');
                  toast.success('Token saved');
                })
              }
            >
              Save
            </Button>
            {has && (
              <Button variant="ghost" onClick={() => void api.setHfToken(null).then(() => setHas(false))}>
                Remove
              </Button>
            )}
          </div>
        </Field>
      </div>
    </section>
  );
}

/** Getting new models: curated picks for this machine, and all of Hugging Face's GGUFs. */
export function DownloadView() {
  const [query, setQuery] = useState('');
  const debounced = useDebounced(query, 350);
  const [results, setResults] = useState<HfRepo[] | null>(null);
  const [searching, setSearching] = useState(false);
  const [repo, setRepo] = useState<string | null>(null);

  useEffect(() => {
    const q = debounced.trim();
    if (!q) {
      setResults(null);
      return;
    }
    setSearching(true);
    api
      .hfSearch(q)
      .then(setResults, (e) => toast.error('Search failed', errorText(e)))
      .finally(() => setSearching(false));
  }, [debounced]);

  return (
    <div className={s.scroll}>
      <Jobs />
      {repo ? (
        <RepoFiles repo={repo} onBack={() => setRepo(null)} />
      ) : (
        <>
          <Recommended />
          <section className={s.section}>
            <h3 className={s.sectionTitle}>Search Hugging Face</h3>
            <TextField
              icon={Search}
              placeholder="Model name, e.g. Qwen3.5 9B, gemma-4, phi"
              value={query}
              onChange={(e) => setQuery(e.target.value)}
              trailing={searching ? <Spinner size={14} /> : undefined}
            />
            {results && results.length === 0 && (
              <EmptyState compact title="No GGUF models found" description="Try a shorter name." />
            )}
            {results && results.length > 0 && (
              <div className={s.rows} style={{ marginTop: 12 }}>
                {results.map((r) => (
                  <div
                    key={r.id}
                    className={`${s.row} ${s.rowButton}`}
                    role="button"
                    tabIndex={0}
                    onClick={() => setRepo(r.id)}
                    onKeyDown={(e) => e.key === 'Enter' && setRepo(r.id)}
                  >
                    <div className={s.rowMain}>
                      <div className={s.rowTitle}>
                        {r.id}
                        {r.preferred && (
                          <Badge
                            tone="accent"
                            icon={BadgeCheck}
                            title="Quantized by unsloth, Demido's preferred publisher"
                          >
                            unsloth
                          </Badge>
                        )}
                      </div>
                      <div className={s.rowMeta}>
                        <Download size={11} /> {formatCount(r.downloads)} · <Heart size={11} /> {formatCount(r.likes)}
                        {r.lastModified ? ` · updated ${r.lastModified.slice(0, 10)}` : ''}
                      </div>
                    </div>
                  </div>
                ))}
              </div>
            )}
          </section>
          <HfToken />
        </>
      )}
    </div>
  );
}
