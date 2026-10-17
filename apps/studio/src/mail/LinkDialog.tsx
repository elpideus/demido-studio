import { useEffect, useState } from 'react';
import { ExternalLink, ShieldAlert } from 'lucide-react';
import { Button, Checkbox, Dialog } from '@demido/ui';

import { linkHost } from './emailHtml';
import styles from './MailWindow.module.css';
import { api, errorText } from '@/lib/api';
import { useApp } from '@/stores/app';
import { toast } from '@/stores/toasts';

/** Opens a link from an email in the browser, or a mail link in the mail app. */
export function openLink(url: string) {
  void api.openUrl(url).catch((e) => toast.error('Could not open the link', errorText(e)));
}

/** Whether a link from an email asks before it opens: web links do, unless the person said not to. */
export function asksFirst(url: string): boolean {
  return /^https?:/i.test(url) && (useApp.getState().settings?.mailConfirmLinks ?? true);
}

/**
 * Asks before a web link from an email opens in the browser: it can lead to a page made to look
 * like another, to take a password, or to a harmful download. "Don't ask again" stops asking for
 * every email; the Mail window's toolbar starts it again.
 */
export function LinkDialog({ url, onClose }: { url: string | null; onClose: () => void }) {
  const patchSettings = useApp((s) => s.patchSettings);
  const [remember, setRemember] = useState(false);

  useEffect(() => setRemember(false), [url]);

  const open = () => {
    if (!url) return;
    onClose();
    if (remember) void patchSettings({ mailConfirmLinks: false });
    openLink(url);
  };

  return (
    <Dialog
      open={url !== null}
      onClose={onClose}
      icon={<ShieldAlert size={18} />}
      title="Open this link?"
      description="Links in emails can lead to fake sites made to steal passwords, or to harmful downloads. Open it only if you trust the sender and the site."
      footer={
        <>
          <Button variant="ghost" onClick={onClose}>
            Cancel
          </Button>
          <Button variant="primary" icon={ExternalLink} onClick={open}>
            Open in browser
          </Button>
        </>
      }
    >
      {url && (
        <div className={styles.linkBox}>
          <div className={styles.linkHost}>{linkHost(url)}</div>
          <div className={styles.linkUrl}>{url}</div>
        </div>
      )}
      <Checkbox
        checked={remember}
        onChange={setRemember}
        label="Don't ask again"
        description="Links in emails open right away. The shield in the Mail toolbar turns asking back on."
      />
    </Dialog>
  );
}
