import { useEffect, useState } from 'react';
import { CheckCircle2, CircleAlert, Cpu, Power, ScrollText, Wrench } from 'lucide-react';
import { Badge, Button, Dialog, Spinner, Switch } from '@demido/ui';

import { api, errorText } from '@/lib/api';
import { useApp } from '@/stores/app';
import { unloadModel, useLoadedModelAnswering, useModels } from '@/stores/models';
import { toast } from '@/stores/toasts';
import { Group, PageHeader, Row, Value } from './layout';
import s from './settings.module.css';

function RuntimeLogs({ open, onClose }: { open: boolean; onClose: () => void }) {
  const [lines, setLines] = useState<string[]>([]);
  useEffect(() => {
    if (open) void api.runtimeLogs().then(setLines);
  }, [open]);
  return (
    <Dialog
      open={open}
      onClose={onClose}
      title="Runtime log"
      description="The last lines llama-server printed."
      width={760}
    >
      <pre className={s.logs}>{lines.length ? lines.join('\n') : 'Nothing logged yet.'}</pre>
    </Dialog>
  );
}

export function RuntimeTab() {
  const info = useApp((st) => st.info);
  const settings = useApp((st) => st.settings);
  const patch = useApp((st) => st.patchSettings);
  const runtime = useModels((st) => st.runtime);
  const answering = useLoadedModelAnswering();
  const [logsOpen, setLogsOpen] = useState(false);

  if (!info || !settings) return null;
  const badge = {
    idle: <Badge>Idle</Badge>,
    loading: <Badge tone="info">Loading</Badge>,
    ready: <Badge tone="accent">Ready</Badge>,
    error: <Badge tone="danger">Error</Badge>,
    unavailable: <Badge tone="warning">Not installed</Badge>,
  }[runtime?.state ?? 'idle'];

  return (
    <div className={s.page}>
      <PageHeader page="runtime" />
      <div className={s.scroll}>
        <Group title="Status" aside={badge}>
          <Row
            setting="runtime-status"
            icon={<Cpu size={18} strokeWidth={1.8} className={s.ok} />}
            label={
              <>
                {runtime?.state === 'loading' && <Spinner size={12} />}
                {runtime?.modelName ?? 'No model loaded'}
              </>
            }
            description={
              runtime?.state === 'error'
                ? runtime.message
                : runtime?.state === 'ready'
                  ? `${runtime.contextLength?.toLocaleString()} token context${runtime.loadSeconds ? ` · loaded in ${runtime.loadSeconds.toFixed(1)}s` : ''}`
                  : info.backend
                    ? `${info.backend} backend`
                    : 'The local runtime is not installed.'
            }
          >
            <Button size="sm" variant="ghost" icon={ScrollText} onClick={() => setLogsOpen(true)}>
              Log
            </Button>
            {runtime?.state === 'ready' && (
              <Button
                size="sm"
                variant="secondary"
                icon={Power}
                disabled={answering}
                title={answering ? 'It is writing an answer' : undefined}
                onClick={unloadModel}
              >
                Unload
              </Button>
            )}
            {info.setupAvailable && (
              <Button
                size="sm"
                variant="secondary"
                icon={Wrench}
                onClick={() => void api.openSetup().catch((e) => toast.error('Could not open setup', errorText(e)))}
              >
                {info.backend ? 'Change…' : 'Set up'}
              </Button>
            )}
          </Row>
        </Group>

        {info.components.length > 0 && (
          <Group
            title="Components"
            setting="runtime-components"
            footer={
              info.components.some((c) => !c.installed)
                ? info.setupAvailable
                  ? 'Set up the AI runtime above to add what is missing.'
                  : 'Run the installer again to add what is missing.'
                : undefined
            }
          >
            {info.components.map((c) => (
              <Row
                key={c.name}
                icon={
                  c.installed ? (
                    <CheckCircle2 size={16} strokeWidth={2} className={s.ok} />
                  ) : (
                    <CircleAlert size={16} strokeWidth={2} className={s.missing} />
                  )
                }
                label={c.name}
                className={s.valueRow}
              >
                <Value>{c.installed ? (c.version ?? 'Installed') : 'Not installed'}</Value>
              </Row>
            ))}
          </Group>
        )}

        <Group title="Startup">
          <Row
            setting="preload"
            label="Load the default model when Demido starts"
            description="The first answer arrives faster. Uses GPU memory while the app is open."
          >
            <Switch
              checked={settings.preloadDefaultModel}
              onChange={(v) => void patch({ preloadDefaultModel: v })}
              label="Preload default model"
            />
          </Row>
        </Group>

        <Group title="This computer" setting="hardware">
          <Row label="Graphics" className={s.valueRow}>
            <Value>
              {info.gpu ?? 'No dedicated GPU found'}
              {info.vramGb ? ` · ${info.vramGb} GB` : ''}
            </Value>
          </Row>
          <Row label="Processor" className={s.valueRow}>
            <Value title={info.cpu}>{info.cpu}</Value>
          </Row>
          <Row label="Memory" className={s.valueRow}>
            <Value>{info.memoryGb} GB</Value>
          </Row>
          <Row label="System" className={s.valueRow}>
            <Value title={info.os}>{info.os}</Value>
          </Row>
        </Group>
      </div>
      <RuntimeLogs open={logsOpen} onClose={() => setLogsOpen(false)} />
    </div>
  );
}
