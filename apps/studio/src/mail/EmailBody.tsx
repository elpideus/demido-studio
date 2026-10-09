import { useEffect, useMemo, useRef } from 'react';
import { openUrl } from '@tauri-apps/plugin-opener';

import { linkTarget, prepareEmailHtml, splitLinks } from './emailHtml';
import styles from './MailWindow.module.css';

function openLink(href: string | null | undefined) {
  const url = linkTarget(href);
  if (url) void openUrl(url).catch(() => {});
}

/**
 * The HTML of an email in a sandboxed frame that runs no scripts. The frame is as tall as the
 * email, so the reader scrolls it together with the header; links open in the browser.
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
      const click = (e: MouseEvent) => {
        const anchor = (e.target as Element | null)?.closest?.('a[href]');
        if (!anchor || (e.type === 'auxclick' && e.button !== 1)) return;
        e.preventDefault();
        const href = anchor.getAttribute('href') ?? '';
        if (href.startsWith('#')) {
          const id = decodeURIComponent(href.slice(1));
          (doc.getElementById(id) ?? doc.getElementsByName(id)[0])?.scrollIntoView({ block: 'start' });
        } else {
          openLink(href);
        }
      };
      doc.addEventListener('click', click);
      doc.addEventListener('auxclick', click);
      // Without scripts the email only changes height as its images arrive and as the pane is
      // resized. The frame's own height is not watched: an email sized to the viewport (100vh)
      // would grow it forever.
      doc.addEventListener('load', measure, true);
      doc.addEventListener('error', measure, true);
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
        doc.removeEventListener('click', click);
        doc.removeEventListener('auxclick', click);
        doc.removeEventListener('load', measure, true);
        doc.removeEventListener('error', measure, true);
        observer.disconnect();
        timers.forEach((t) => window.clearTimeout(t));
      };
    };
    iframe.addEventListener('load', attach);
    if (iframe.contentDocument?.readyState === 'complete') attach();
    return () => {
      iframe.removeEventListener('load', attach);
      cleanup();
    };
  }, [prepared.doc]);

  return (
    <iframe
      ref={frame}
      className={styles.frame}
      title="Email"
      sandbox="allow-same-origin allow-popups allow-popups-to-escape-sandbox"
      referrerPolicy="no-referrer"
      srcDoc={prepared.doc}
    />
  );
}

/** A plain-text email, with its web addresses as links. */
export function TextBody({ text }: { text: string }) {
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
              openLink(part.href);
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
