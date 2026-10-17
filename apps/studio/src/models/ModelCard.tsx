import { type ComponentProps, type MouseEvent, memo, useRef, useState } from 'react';
import ReactMarkdown, { type Components } from 'react-markdown';
import rehypeHighlight from 'rehype-highlight';
import rehypeKatex from 'rehype-katex';
import rehypeRaw from 'rehype-raw';
import rehypeSanitize, { type Options as SanitizeSchema, defaultSchema } from 'rehype-sanitize';
import remarkGfm from 'remark-gfm';
import remarkMath from 'remark-math';
import { ImageOff } from 'lucide-react';
import { cx } from '@demido/ui';
import 'katex/dist/katex.min.css';

import { CodeBlock } from '@/chat/Markdown';
import chat from '@/chat/Markdown.module.css';
import { api } from '@/lib/api';
import { resolveCardUrl, resolveSrcSet, slug } from './cardUrls';
import styles from './ModelCard.module.css';

/**
 * What a card's own HTML may keep: GitHub's rules for READMEs (no scripts, no styles, no event
 * handlers, web and mail addresses only), plus the few presentational tags model cards use.
 * Cards are written by whoever published the repository, and this page can reach the backend.
 */
const schema: SanitizeSchema = {
  ...defaultSchema,
  tagNames: [...(defaultSchema.tagNames ?? []), 'center', 'figure', 'figcaption', 'u', 'mark'],
};

/** The part of a HTML tree the callout step reads: nodes, their children, their text. */
interface Node {
  type: string;
  tagName?: string;
  value?: string;
  properties?: Record<string, unknown>;
  children?: Node[];
}

const ALERTS = ['note', 'tip', 'important', 'warning', 'caution'] as const;
type Alert = (typeof ALERTS)[number];

/**
 * GitHub's callouts, which many cards use: a quote opening with `[!NOTE]` (or TIP, IMPORTANT,
 * WARNING, CAUTION) loses the marker and is drawn as a note of that kind. Runs after the sanitizer,
 * on a tree it already cleaned, and adds nothing but a data attribute.
 */
function rehypeAlerts() {
  const visit = (node: Node) => {
    if (node.type === 'element' && node.tagName === 'blockquote') {
      const first = node.children?.find((c) => c.type === 'element');
      const text = first?.tagName === 'p' ? first.children?.[0] : undefined;
      const match = text?.type === 'text' ? /^\s*\[!(\w+)\]\s*/.exec(text.value ?? '') : null;
      const kind = match?.[1]?.toLowerCase() as Alert | undefined;
      if (text && match && kind && ALERTS.includes(kind)) {
        text.value = (text.value ?? '').slice(match[0].length);
        node.properties = { ...node.properties, dataAlert: kind };
      }
    }
    node.children?.forEach(visit);
  };
  return (tree: Node) => visit(tree);
}

const ALERT_LABELS: Record<Alert, string> = {
  note: 'Note',
  tip: 'Tip',
  important: 'Important',
  warning: 'Warning',
  caution: 'Caution',
};

const remarkPlugins = [remarkGfm, remarkMath];
// Sanitized as soon as the HTML is parsed; KaTeX and highlighting add their own classes after.
const rehypePlugins: ComponentProps<typeof ReactMarkdown>['rehypePlugins'] = [
  rehypeRaw,
  [rehypeSanitize, schema],
  rehypeAlerts,
  rehypeKatex,
  [rehypeHighlight, { detect: false, ignoreMissing: true }],
];

/** A picture from the card: shown, or, when it cannot be loaded, said so where it would have been. */
function CardImage({ src, alt, width, height }: { src: string; alt?: string; width?: string; height?: string }) {
  const [broken, setBroken] = useState(false);
  if (broken) {
    return (
      <span className={styles.broken} title={src}>
        <ImageOff size={13} aria-hidden />
        {alt || 'Picture'}
      </span>
    );
  }
  return (
    <img
      src={src}
      alt={alt ?? ''}
      width={width}
      height={height}
      loading="lazy"
      decoding="async"
      referrerPolicy="no-referrer"
      onError={() => setBroken(true)}
    />
  );
}

function components(repo: string, scrollTo: (anchor: string) => void): Components {
  return {
    pre: ({ children }) => <CodeBlock>{children}</CodeBlock>,
    a: ({ href, children, title }) => {
      const target = resolveCardUrl(href, repo, 'link');
      const open = (e: MouseEvent) => {
        e.preventDefault();
        if (!target) return;
        if (target.startsWith('#')) scrollTo(target.slice(1));
        else void api.openUrl(target);
      };
      return (
        <a
          href={target ?? undefined}
          title={title ?? (target && !target.startsWith('#') ? target : undefined)}
          onClick={open}
        >
          {children}
        </a>
      );
    },
    img: ({ src, alt, width, height }) => {
      const resolved = resolveCardUrl(typeof src === 'string' ? src : undefined, repo, 'image');
      if (!resolved) return null;
      return (
        <CardImage
          src={resolved}
          alt={alt}
          width={width === undefined ? undefined : String(width)}
          height={height === undefined ? undefined : String(height)}
        />
      );
    },
    source: ({ srcSet, media, type }) => <source srcSet={resolveSrcSet(srcSet, repo)} media={media} type={type} />,
    blockquote: ({ children, node }) => {
      const kind = node?.properties?.dataAlert as Alert | undefined;
      if (!kind) return <blockquote>{children}</blockquote>;
      return (
        <div className={cx(styles.alert, styles[`alert-${kind}`])} role="note">
          <span className={styles.alertLabel}>{ALERT_LABELS[kind]}</span>
          {children}
        </div>
      );
    },
    table: ({ children }) => (
      <div className={chat.tableWrap}>
        <table>{children}</table>
      </div>
    ),
  };
}

/**
 * A Hugging Face model card, whole: its Markdown and the HTML in it, pictures included, drawn in
 * Demido's own type and colours. Links open in the browser; a link to a section scrolls to it.
 */
export const ModelCard = memo(function ModelCard({ markdown, repo }: { markdown: string; repo: string }) {
  const root = useRef<HTMLDivElement>(null);
  const scrollTo = (anchor: string) => {
    const wanted = decodeURIComponent(anchor).toLowerCase();
    const el =
      root.current?.querySelector(
        `[id="user-content-${CSS.escape(wanted)}"], [name="user-content-${CSS.escape(wanted)}"]`,
      ) ??
      [...(root.current?.querySelectorAll('h1, h2, h3, h4, h5, h6') ?? [])].find(
        (h) => slug(h.textContent ?? '') === wanted,
      );
    el?.scrollIntoView({ behavior: 'smooth', block: 'start' });
  };
  return (
    <div ref={root} className={cx(chat.md, styles.card, 'selectable')}>
      <ReactMarkdown
        remarkPlugins={remarkPlugins}
        rehypePlugins={rehypePlugins}
        components={components(repo, scrollTo)}
      >
        {markdown}
      </ReactMarkdown>
    </div>
  );
});
