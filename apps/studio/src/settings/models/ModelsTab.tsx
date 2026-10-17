import { useEffect, useState } from 'react';
import { Boxes } from 'lucide-react';
import { Button } from '@demido/ui';

import { type WindowState, useWindows } from '@/stores/windows';
import s from '../settings.module.css';
import { ModelEditor } from './ModelEditor';
import { ModelList } from './ModelList';

export function ModelsTab({ win }: { win: WindowState }) {
  const setProps = useWindows((st) => st.setProps);
  const open = useWindows((st) => st.open);
  const [editing, setEditing] = useState<string | null>(typeof win.props.model === 'string' ? win.props.model : null);
  // Kept here so the searches and unfolded groups are still there after editing a model they found.
  const [query, setQuery] = useState('');
  const [groupQueries, setGroupQueries] = useState<Record<string, string | null>>({});
  const [expandedGroups, setExpandedGroups] = useState<Record<string, boolean>>({});

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
          <p className={s.subtitle}>Choose which models appear in the chat and tune how they answer.</p>
        </div>
        <div className={s.headerActions}>
          <Button variant="secondary" icon={Boxes} onClick={() => open('models')}>
            Get models
          </Button>
        </div>
      </header>
      <ModelList
        query={query}
        onQueryChange={setQuery}
        expandedGroups={expandedGroups}
        onToggleGroup={(group) => setExpandedGroups((prev) => ({ ...prev, [group]: !prev[group] }))}
        groupQueries={groupQueries}
        onGroupQueryChange={(group, value) => setGroupQueries((prev) => ({ ...prev, [group]: value }))}
        onEdit={setEditing}
        onDownload={() => open('models')}
      />
    </div>
  );
}
