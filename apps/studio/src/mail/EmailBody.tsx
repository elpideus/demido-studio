import { useEffect, useMemo, useRef, useState } from 'react';
import { Spinner, cx } from '@demido/ui';

import { prepareEmailHtml, splitLinks } from './emailHtml';
import styles from './MailWindow.module.css';

/** The longest an email waits for its images before it shows; the rest arrive in place. */
const IMAGES_WAIT_MS = 2500;

/**
 * The HTML of an email in a sandboxed frame that runs no scripts. The frame is as tall as the
 * email, so the reader scrolls it together with the header; its links come back as
 * `app://new-window` events (see emailHtml.ts). It shows once its images have arrived, so the
 * email does not appear without them and then jump.
 */
export function HtmlBody({
  html,
  showImages,
  onRemote,
}: {
  html: string;
  showImages: boolean;
  onRemote: (remote: boolean) => void;
}) {
  const frame = useRef<HTMLIFrameElement>(null);
  const prepared = useMemo(() => prepareEmailHtml(html, showImages), [html, showImages]);
  const [shown, setShown] = useState<string | null>(null);
  const ready = shown === prepared.doc;

  useEffect(() => onRemote(prepared.remote), [prepared.remote, onRemote]);

  useEffect(() => {
    const iframe = frame.current;
    if (!iframe) return;
    let cleanup = () => {};
    const attach = () => {
      cleanup();
      const doc = iframe.contentDocument;
      if (!doc?.documentElement) return;
      const measure = () => {
        const root = doc.documentElement;
        const height = Math.max(
          root.getBoundingClientRect().height,
          root.scrollHeight > root.clientHeight ? root.scrollHeight : 0,
        );
        iframe.style.height = `${Math.ceil(height)}px`;
      };
      // Without scripts the email only changes height as its images arrive (the frame's load) and
      // as the pane is resized. Nothing listens inside the frame: WebKitGTK runs none of the page's
      // listeners there, and logs each one it blocks. The frame's own height is not watched: an
      // email sized to the viewport (100vh) would grow it forever.
      let width = iframe.clientWidth;
      const observer = new ResizeObserver(() => {
        if (iframe.clientWidth === width) return;
        width = iframe.clientWidth;
        measure();
      });
      observer.observe(iframe);
      const timers = [window.setTimeout(measure, 120), window.setTimeout(measure, 1200)];
      measure();
      cleanup = () => {
        observer.disconnect();
        timers.forEach((t) => window.clearTimeout(t));
      };
    };
    // Until the email is parsed the frame holds an empty page, which is neither measured nor
    // shown. The frame's load waits for every image; a slow one only holds the email back so long.
    const parsed = () => iframe.contentDocument?.URL === 'about:srcdoc';
    const loaded = () => {
      attach();
      setShown(prepared.doc);
    };
    iframe.addEventListener('load', loaded);
    if (parsed() && iframe.contentDocument?.readyState === 'complete') loaded();
    const late = window.setTimeout(() => {
      if (parsed()) attach();
      setShown(prepared.doc);
    }, IMAGES_WAIT_MS);
    return () => {
      iframe.removeEventListener('load', loaded);
      window.clearTimeout(late);
      cleanup();
    };
  }, [prepared.doc]);

  return (
    <div className={styles.frameBox}>
      {!ready && (
        <div className={styles.frameLoading}>
          <Spinner />
        </div>
      )}
      <iframe
        ref={frame}
        className={cx(styles.frame, !ready && styles.frameHidden)}
        title="Email"
        sandbox="allow-same-origin allow-popups"
        referrerPolicy="no-referrer"
        srcDoc={prepared.doc}
      />
    </div>
  );
}

/** A plain-text email, with its web addresses as links that go to `onLink`. */
export function TextBody({ text, onLink }: { text: string; onLink: (url: string) => void }) {
  const parts = useMemo(() => splitLinks(text), [text]);
  return (
    <div className={styles.textBody}>
      {parts.map((part, i) =>
        part.href ? (
          <a
            key={i}
            href={part.href}
            title={part.href}
            onClick={(e) => {
              e.preventDefault();
              onLink(part.href!);
            }}
          >
            {part.text}
          </a>
        ) : (
          <span key={i}>{part.text}</span>
        ),
      )}
    </div>
  );
}
