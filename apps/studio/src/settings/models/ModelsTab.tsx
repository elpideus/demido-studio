import { useEffect, useState } from 'react';
import { Download, HardDrive } from 'lucide-react';
import { Badge, SegmentedControl } from '@demido/ui';

import { useModels } from '@/stores/models';
import { type WindowState, useWindows } from '@/stores/windows';
import s from '../settings.module.css';
import { DownloadView } from './DownloadView';
import { ModelEditor } from './ModelEditor';
import { ModelList } from './ModelList';

type View = 'installed' | 'download';

export function ModelsTab({ win }: { win: WindowState }) {
  const setProps = useWindows((st) => st.setProps);
  const downloads = useModels((st) => st.downloads);
  const view: View = win.props.view === 'download' ? 'download' : 'installed';
  const [editing, setEditing] = useState<string | null>(typeof win.props.model === 'string' ? win.props.model : null);
  const active = downloads.filter((d) => d.state === 'downloading' || d.state === 'queued').length;

  useEffect(() => {
    if (typeof win.props.model === 'string') setEditing(win.props.model);
  }, [win.props.model]);

  if (editing) {
    return (
      <ModelEditor
        id={editing}
        onBack={() => {
          setEditing(null);
          setProps(win.id, { model: undefined });
        }}
      />
    );
  }

  return (
    <div className={s.page}>
      <header className={s.header}>
        <div className={s.headerText}>
          <h2 className={s.title}>Models</h2>
          <p className={s.subtitle}>Choose which models appear in the chat, tune how they answer, or get new ones.</p>
        </div>
        <div className={s.headerActions}>
          <SegmentedControl
            value={view}
            onChange={(v) => setProps(win.id, { view: v })}
            options={[
              { value: 'installed', label: 'Installed', icon: HardDrive },
              { value: 'download', label: active ? `Download (${active})` : 'Download', icon: Download },
            ]}
          />
        </div>
      </header>
      {view === 'installed' ? (
        <ModelList onEdit={setEditing} onDownload={() => setProps(win.id, { view: 'download' })} />
      ) : (
        <DownloadView />
      )}
      {active > 0 && view === 'installed' && (
        <div className={s.muted} style={{ padding: '0 24px 12px' }}>
          <Badge tone="info">{active} downloading</Badge>
        </div>
      )}
    </div>
  );
}
