import { AtSign, ChevronRight, ExternalLink } from 'lucide-react';
import { Badge, Button, Switch, cx } from '@demido/ui';

import { useApp } from '@/stores/app';
import { useMail } from '@/stores/mail';
import { useWindows } from '@/stores/windows';
import { Group, PageHeader, Row } from './layout';
import s from './settings.module.css';

export function MailTab() {
  const settings = useApp((st) => st.settings);
  const patch = useApp((st) => st.patchSettings);
  const accounts = useMail((st) => st.accounts);
  if (!settings) return null;
  const openMail = (account?: string) => useWindows.getState().open('mail', account ? { account, folder: null } : {});

  return (
    <div className={s.page}>
      <PageHeader page="mail" />
      <div className={s.scroll}>
        <Group title="Reading" footer="These hold for every account. The Mail window’s account menu has them too.">
          <Row
            setting="mail-images"
            label="Load images automatically"
            description="Off, images from the internet stay hidden until you ask: loading them can tell the sender you read the email."
          >
            <Switch
              checked={settings.mailShowImages ?? false}
              onChange={(v) => void patch({ mailShowImages: v })}
              label="Load images automatically"
            />
          </Row>
          <Row setting="mail-mark-read" label="Mark as read when opened">
            <Switch
              checked={settings.mailMarkRead ?? true}
              onChange={(v) => void patch({ mailMarkRead: v })}
              label="Mark as read when opened"
            />
          </Row>
          <Row
            setting="mail-links"
            label="Ask before opening links"
            description="Shows the address and waits for your OK before the browser opens it."
          >
            <Switch
              checked={settings.mailConfirmLinks ?? true}
              onChange={(v) => void patch({ mailConfirmLinks: v })}
              label="Ask before opening links"
            />
          </Row>
        </Group>

        <Group
          title="Accounts"
          setting="mail-accounts"
          aside={
            <Button size="sm" variant="ghost" icon={ExternalLink} onClick={() => openMail()}>
              Open Mail
            </Button>
          }
          footer="Add, rename or remove accounts from the account menu in the Mail window."
        >
          {accounts.length === 0 ? (
            <Row label="No account yet" description="Connect your email in the Mail window.">
              <Button size="sm" variant="secondary" onClick={() => openMail()}>
                Connect
              </Button>
            </Row>
          ) : (
            accounts.map((a) => (
              <div
                key={a.id}
                role="button"
                tabIndex={0}
                className={cx(s.row, s.rowButton)}
                onClick={() => openMail(a.id)}
                onKeyDown={(e) => e.key === 'Enter' && openMail(a.id)}
              >
                <span className={s.rowIcon}>
                  <AtSign size={18} strokeWidth={1.8} aria-hidden />
                </span>
                <div className={s.rowMain}>
                  <div className={s.rowTitle}>{a.nickname ?? a.email}</div>
                  <div className={s.rowMeta}>{a.nickname ? `${a.email} · ${a.host}` : a.host}</div>
                </div>
                <div className={s.rowActions}>
                  {a.needsPassword ? (
                    <Badge tone="warning">Needs password</Badge>
                  ) : (
                    a.error && <Badge tone="danger">Can’t connect</Badge>
                  )}
                  <ChevronRight size={16} className={s.chevron} aria-hidden />
                </div>
              </div>
            ))
          )}
        </Group>
      </div>
    </div>
  );
}
