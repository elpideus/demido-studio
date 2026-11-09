import { useState } from 'react';
import { FolderOpen, RefreshCw } from 'lucide-react';
import { Button, Dialog, Logo } from '@demido/ui';

import { api } from '@/lib/api';
import { useApp } from '@/stores/app';
import { Group, PageHeader, Row, useSettingsNav } from './layout';
import s from './settings.module.css';

export function AboutTab() {
  const info = useApp((st) => st.info);
  const go = useSettingsNav();
  const [noticeOpen, setNoticeOpen] = useState(false);
  if (!info) return null;

  return (
    <div className={s.page}>
      <PageHeader page="about" />
      <div className={s.scroll}>
        <section className={s.section}>
          <div className={s.identity} data-setting="about-version">
            <span className={s.identityLogo}>
              <Logo size={30} />
            </span>
            <div className={s.rowMain}>
              <div className={s.identityName}>Demido Studio</div>
              <div className={s.rowMeta}>
                Version {info.version}
                {info.dev ? ' · Development build' : ''}
              </div>
            </div>
            <Button
              size="sm"
              variant="secondary"
              icon={RefreshCw}
              onClick={() => go({ tab: 'updates', focus: 'update-status' })}
            >
              Check for updates
            </Button>
          </div>
        </section>

        <Group title="Storage">
          <Row setting="data-folder" label="Your data" description={<span className={s.mono}>{info.dataDir}</span>}>
            <Button size="sm" variant="secondary" icon={FolderOpen} onClick={() => void api.openPath(info.dataDir)}>
              Open
            </Button>
          </Row>
          {info.installDir && (
            <Row
              setting="install-folder"
              label="Installation"
              description={<span className={s.mono}>{info.installDir}</span>}
            >
              <Button
                size="sm"
                variant="secondary"
                icon={FolderOpen}
                onClick={() => void api.openPath(info.installDir!)}
              >
                Open
              </Button>
            </Row>
          )}
        </Group>

        <Group title="Legal">
          <Row
            setting="safety-notice"
            label="Safety notice"
            description="What AI models can get wrong, and what to keep an eye on."
          >
            <Button size="sm" variant="secondary" onClick={() => setNoticeOpen(true)}>
              Read
            </Button>
          </Row>
          <Row setting="license" label="License" description="Free software under the GNU GPL v3 or later." />
        </Group>
      </div>
      <Dialog open={noticeOpen} onClose={() => setNoticeOpen(false)} title="Safety notice" width={520}>
        Demido Studio is still under development. AI models can be wrong, misbehave, or be manipulated by content they
        read (prompt injection) into doing things you did not ask for. Review what the assistant does, and only allow
        code you are comfortable with. Nothing here is financial or other professional advice. The software is provided
        as is, without warranty, and its author accepts no responsibility for any consequence of its use.
      </Dialog>
    </div>
  );
}
