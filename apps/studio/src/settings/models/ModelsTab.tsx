import { useEffect, useState } from 'react';
import { Download, HardDrive } from 'lucide-react';
import { SegmentedControl } from '@demido/ui';

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
  // Kept here so the searches are still there after editing a model they found.
  const [query, setQuery] = useState('');
  const [groupQueries, setGroupQueries] = useState<Record<string, string | null>>({});
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
        <ModelList
          query={query}
          onQueryChange={setQuery}
          groupQueries={groupQueries}
          onGroupQueryChange={(group, value) => setGroupQueries((prev) => ({ ...prev, [group]: value }))}
          onEdit={setEditing}
          onDownload={() => setProps(win.id, { view: 'download' })}
        />
      ) : (
        <DownloadView />
      )}
    </div>
  );
}
