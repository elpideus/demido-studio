import { useState } from 'react';
import { ExternalLink } from 'lucide-react';
import { Button, Select, Switch, TextField } from '@demido/ui';

import { api, errorText } from '@/lib/api';
import type { SearchEngine } from '@/lib/types';
import { useApp } from '@/stores/app';
import { toast } from '@/stores/toasts';
import { useWindows } from '@/stores/windows';
import { Group, PageHeader, Row } from './layout';
import s from './settings.module.css';
import { searxngAddress } from './webView';

const ENGINES: Array<{ value: SearchEngine; label: string }> = [
  { value: 'duckduckgo', label: 'DuckDuckGo' },
  { value: 'brave', label: 'Brave Search' },
  { value: 'bing', label: 'Bing' },
  { value: 'google', label: 'Google' },
];

function SearxngRow() {
  const saved = useApp((st) => st.settings?.searxngUrl ?? '');
  const patch = useApp((st) => st.patchSettings);
  const [draft, setDraft] = useState<string | null>(null);
  const commit = () => {
    if (draft === null) return;
    const address = searxngAddress(draft);
    if (address === undefined) {
      toast.error('Not a server address', 'Type its address, such as https://searx.example.org.');
      return;
    }
    setDraft(null);
    if (address !== (saved || null)) void patch({ searxngUrl: address });
  };
  return (
    <Row
      setting="searxng"
      label="SearXNG server"
      description="Searches ask this server first: one you run, or one you trust. Empty, they go to DuckDuckGo, Brave and Bing."
    >
      <TextField
        size="sm"
        className={s.wide}
        aria-label="SearXNG server"
        placeholder="https://searx.example.org"
        spellCheck={false}
        value={draft ?? saved}
        onChange={(e) => setDraft(e.target.value)}
        onBlur={commit}
        onKeyDown={(e) => {
          if (e.key === 'Enter') e.currentTarget.blur();
          else if (e.key === 'Escape') setDraft(null);
        }}
      />
    </Row>
  );
}

function ClearDataRow() {
  const [asking, setAsking] = useState(false);
  const [busy, setBusy] = useState(false);
  const clear = async () => {
    setBusy(true);
    try {
      await api.browserClearData();
      toast.success('Browsing data cleared');
      setAsking(false);
    } catch (e) {
      toast.error('Could not clear the browsing data', errorText(e));
    } finally {
      setBusy(false);
    }
  };
  return (
    <Row
      setting="browser-data"
      label="Browsing data"
      description="Cookies, sign-ins and saved files of every site the browser opened. Clearing them signs you and the assistant out everywhere."
    >
      {asking ? (
        <>
          <Button size="sm" variant="ghost" disabled={busy} onClick={() => setAsking(false)}>
            Cancel
          </Button>
          <Button size="sm" variant="danger" loading={busy} onClick={() => void clear()}>
            Clear everything
          </Button>
        </>
      ) : (
        <Button size="sm" variant="secondary" onClick={() => setAsking(true)}>
          Clear…
        </Button>
      )}
    </Row>
  );
}

export function WebTab() {
  const settings = useApp((st) => st.settings);
  const patch = useApp((st) => st.patchSettings);
  if (!settings) return null;

  return (
    <div className={s.page}>
      <PageHeader page="web" />
      <div className={s.scroll}>
        <Group
          title="Searching"
          footer="Searches and pages go to the sites themselves first. Turn the assistant’s web tools on or off in Permissions."
        >
          <SearxngRow />
          <Row
            setting="web-exa"
            label="Ask Exa when a site refuses"
            description="When the search engines or a page refuse the app, Exa’s free service is sent the search or the page’s address and answers for them. Off, nothing goes to Exa."
          >
            <Switch
              checked={settings.webExa ?? true}
              onChange={(v) => void patch({ webExa: v })}
              label="Ask Exa when a site refuses"
            />
          </Row>
        </Group>

        <Group
          title="Browser"
          aside={
            <Button size="sm" variant="ghost" icon={ExternalLink} onClick={() => useWindows.getState().open('browser')}>
              Open Browser
            </Button>
          }
          footer="The assistant reads pages that need a real browser here, each chat in its own tab, and shows you the tab when a page asks for a person (a captcha or a sign-in). Sites you sign in to stay signed in, for you and the assistant."
        >
          <Row setting="browser-engine" label="Address bar searches with">
            <Select
              size="sm"
              className={s.wide}
              aria-label="Address bar searches with"
              value={settings.browserSearchEngine ?? 'duckduckgo'}
              onChange={(e) => void patch({ browserSearchEngine: e.target.value as SearchEngine })}
              options={ENGINES}
            />
          </Row>
          <ClearDataRow />
        </Group>
      </div>
    </div>
  );
}
