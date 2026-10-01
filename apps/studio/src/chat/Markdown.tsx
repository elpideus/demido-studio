import { memo, useState, type ComponentProps, type ReactNode } from 'react';
import ReactMarkdown, { type Components } from 'react-markdown';
import rehypeHighlight from 'rehype-highlight';
import rehypeKatex from 'rehype-katex';
import remarkGfm from 'remark-gfm';
import remarkMath from 'remark-math';
import { openUrl } from '@tauri-apps/plugin-opener';
import { Check, Copy, Image as ImageIcon } from 'lucide-react';
import { cx } from '@demido/ui';
import 'katex/dist/katex.min.css';

import { fileUrl } from '@/lib/format';
import styles from './Markdown.module.css';
import { prepareMath } from './mathText';

const remarkPlugins = [remarkGfm, remarkMath];
const rehypePlugins: ComponentProps<typeof ReactMarkdown>['rehypePlugins'] = [
  rehypeKatex,
  [rehypeHighlight, { detect: false, ignoreMissing: true }],
];

function textOf(node: ReactNode): string {
  if (typeof node === 'string' || typeof node === 'number') return String(node);
  if (Array.isArray(node)) return node.map(textOf).join('');
  if (node && typeof node === 'object' && 'props' in node) {
    return textOf((node as { props: { children?: ReactNode } }).props.children);
  }
  return '';
}

export function CopyButton({ text, label = 'Copy' }: { text: string; label?: string }) {
  const [copied, setCopied] = useState(false);
  return (
    <button
      type="button"
      className={styles.copy}
      onClick={() => {
        void navigator.clipboard.writeText(text).then(() => {
          setCopied(true);
          window.setTimeout(() => setCopied(false), 1500);
        });
      }}
    >
      {copied ? <Check size={13} aria-hidden /> : <Copy size={13} aria-hidden />}
      {copied ? 'Copied' : label}
    </button>
  );
}

function CodeBlock({ children }: { children?: ReactNode }) {
  const child = Array.isArray(children) ? children[0] : children;
  const className =
    child && typeof child === 'object' && 'props' in child
      ? String((child as { props: { className?: string } }).props.className ?? '')
      : '';
  const language = /language-([\w+-]+)/.exec(className)?.[1] ?? '';
  const code = textOf(children).replace(/\n$/, '');
  return (
    <div className={styles.codeBlock}>
      <div className={styles.codeHeader}>
        <span className={styles.codeLang}>{language || 'text'}</span>
        <CopyButton text={code} />
      </div>
      <pre className={styles.pre}>{children}</pre>
    </div>
  );
}

function makeComponents(workspace: string | null): Components {
  return {
    pre: ({ children }) => <CodeBlock>{children}</CodeBlock>,
    a: ({ href, children }) => (
      <a
        href={href}
        onClick={(e) => {
          e.preventDefault();
          if (href && /^(https?:|mailto:)/i.test(href)) void openUrl(href);
        }}
        title={href}
      >
        {children}
      </a>
    ),
    img: ({ src, alt }) => {
      const raw = typeof src === 'string' ? src : '';
      // A picture on the web is never fetched by itself: a file the model read could make it
      // write one whose address carries the conversation to someone else's server. It opens in
      // the browser when clicked.
      if (/^https?:/i.test(raw)) {
        let host = raw;
        try {
          host = new URL(raw).host;
        } catch {
          // Keep the whole address.
        }
        // A span, not a link: a linked image (a README badge) sits inside the Markdown link, and
        // links do not nest. Its own click opens the picture, and does not reach the link.
        const open = (e: { preventDefault: () => void; stopPropagation: () => void }) => {
          e.preventDefault();
          e.stopPropagation();
          void openUrl(raw);
        };
        return (
          <span
            role="link"
            tabIndex={0}
            className={styles.remoteImage}
            title={raw}
            onClick={open}
            onKeyDown={(e) => {
              if (e.key === 'Enter') open(e);
            }}
          >
            <ImageIcon size={14} strokeWidth={1.8} aria-hidden />
            {alt || 'Image'} ({host})
          </span>
        );
      }
      // Images the assistant saved in the chat's folder are referenced by relative path.
      const resolved =
        /^(data:|asset:|blob:)/i.test(raw) || !workspace
          ? raw
          : (fileUrl(`${workspace}/${raw.replace(/^\.?\//, '')}`) ?? raw);
      return <img src={resolved} alt={alt ?? ''} className={styles.image} loading="lazy" />;
    },
    table: ({ children }) => (
      <div className={styles.tableWrap}>
        <table>{children}</table>
      </div>
    ),
  };
}

interface Props {
  text: string;
  /** Chat workspace folder, for images referenced by relative path. */
  workspace?: string | null;
  className?: string;
}

/** Renders assistant Markdown: GFM tables and lists, KaTeX math, highlighted code. */
export const Markdown = memo(function Markdown({ text, workspace = null, className }: Props) {
  return (
    <div className={cx(styles.md, 'selectable', className)}>
      <ReactMarkdown remarkPlugins={remarkPlugins} rehypePlugins={rehypePlugins} components={makeComponents(workspace)}>
        {prepareMath(text)}
      </ReactMarkdown>
    </div>
  );
});
