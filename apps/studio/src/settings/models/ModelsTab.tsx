import { useState } from 'react';
import { Boxes } from 'lucide-react';
import { Button } from '@demido/ui';

import { type WindowState, useWindows } from '@/stores/windows';
import { PageHeader } from '../layout';
import s from '../settings.module.css';
import { ModelEditor } from './ModelEditor';
import { ModelList } from './ModelList';

export function ModelsTab({ win }: { win: WindowState }) {
  const setProps = useWindows((st) => st.setProps);
  const open = useWindows((st) => st.open);
  // The model whose editor is open, in the window's props so a search result can open one.
  const editing = typeof win.props.model === 'string' ? win.props.model : null;
  const edit = (model: string | undefined) => setProps(win.id, { model });
  // Kept here so the searches and unfolded groups are still there after editing a model they found.
  const [query, setQuery] = useState('');
  const [groupQueries, setGroupQueries] = useState<Record<string, string | null>>({});
  const [expandedGroups, setExpandedGroups] = useState<Record<string, boolean>>({});

  if (editing) {
    return <ModelEditor id={editing} onBack={() => edit(undefined)} />;
  }

  return (
    <div className={s.page}>
      <PageHeader
        page="models"
        actions={
          <Button variant="secondary" icon={Boxes} data-setting="get-models" onClick={() => open('models')}>
            Get models
          </Button>
        }
      />
      <ModelList
        query={query}
        onQueryChange={setQuery}
        expandedGroups={expandedGroups}
        onToggleGroup={(group) => setExpandedGroups((prev) => ({ ...prev, [group]: !prev[group] }))}
        groupQueries={groupQueries}
        onGroupQueryChange={(group, value) => setGroupQueries((prev) => ({ ...prev, [group]: value }))}
        onEdit={edit}
        onDownload={() => open('models')}
      />
    </div>
  );
}
