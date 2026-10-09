// Turns the HTML of an email into a document for a sandboxed frame. The frame runs no scripts
// (its sandbox has no `allow-scripts`) and its content policy loads nothing from the internet
// until the reader asks for images: remote images tell the sender when, and from where, the mail
// was read.

/** What the reader shows for an HTML email. */
export interface PreparedEmail {
  /** The `srcdoc` of the frame. */
  doc: string;
  /** The email loads images or styles from the internet, blocked until they are shown. */
  remote: boolean;
}

/** Elements that run code, load other documents or change how the frame resolves links. */
const DROPPED = 'script, iframe, frame, frameset, object, embed, applet, base, link, meta, noscript, template, portal';

/** Attributes whose value is a URL that can run code when followed or loaded. */
const URL_ATTRIBUTES = ['href', 'src', 'action', 'formaction', 'xlink:href', 'background', 'poster', 'data'];

/** Plain light "paper", like the mail apps show, under the email's own styles. */
const BASE_STYLE = `
html { color-scheme: light; background: #fff; }
body { margin: 0; padding: 18px 22px; color: #1f1f1f; background: #fff;
  font: 14px/1.5 'Segoe UI', system-ui, -apple-system, sans-serif; overflow-wrap: break-word; }
img { max-width: 100%; }
pre { white-space: pre-wrap; }
a { color: #1a62d6; }
blockquote { margin: 0 0 0 4px; padding-left: 12px; border-left: 2px solid #d0d0d0; color: #555; }
`;

/** The content policy of the frame, which only narrows the app's own (the frame inherits it). */
export function contentPolicy(showImages: boolean): string {
  return [
    "default-src 'none'",
    `img-src data:${showImages ? ' https:' : ''}`,
    "style-src 'unsafe-inline'",
    'font-src data:',
    "form-action 'none'",
    "base-uri 'none'",
  ].join('; ');
}

/** A URL the browser fetches from the internet: `https://…`, `http://…` or `//…`. */
export function isRemoteUrl(url: string | null | undefined): boolean {
  return !!url && /^\s*(https?:)?\/\//i.test(url);
}

/** A `srcset` with a remote candidate. */
export function isRemoteSrcset(srcset: string | null | undefined): boolean {
  return !!srcset && srcset.split(',').some((candidate) => isRemoteUrl(candidate.trim()));
}

/** CSS that loads an image from the internet (a remote `url()`). */
export function hasRemoteCss(css: string | null | undefined): boolean {
  return !!css && /url\(\s*['"]?\s*(https?:)?\/\//i.test(css);
}

/** A URL that runs code or opens a document of its own when followed. */
export function isUnsafeUrl(url: string): boolean {
  // Browsers ignore control characters and spaces inside the scheme ("java\tscript:").
  const compact = url.replace(/[\p{Cc}\s]/gu, '').toLowerCase();
  return /^(javascript|vbscript|livescript):/.test(compact) || /^data:(?!image\/)/.test(compact);
}

/** The address a click on a link opens outside the app, or null for links that open nothing. */
export function linkTarget(href: string | null | undefined): string | null {
  if (!href) return null;
  const url = href.trim();
  return /^(https?:\/\/|mailto:)/i.test(url) ? url : null;
}

/** Text split around the web addresses in it, so they can be shown as links. */
export function splitLinks(text: string): Array<{ text: string; href?: string }> {
  const parts: Array<{ text: string; href?: string }> = [];
  const pattern = /\bhttps?:\/\/[^\s<>"]+/gi;
  let last = 0;
  for (let match = pattern.exec(text); match; match = pattern.exec(text)) {
    let url = match[0];
    // Punctuation that ends the sentence, and a closing bracket that is not the URL's own.
    while (/[.,;:!?'"*]$/.test(url) || (/[)\]]$/.test(url) && !hasOpening(url))) url = url.slice(0, -1);
    const start = match.index;
    if (start > last) parts.push({ text: text.slice(last, start) });
    parts.push({ text: url, href: url });
    last = start + url.length;
    pattern.lastIndex = last;
  }
  if (last < text.length) parts.push({ text: text.slice(last) });
  return parts;
}

/** The URL opens as many brackets as it closes, so its last one is its own: `…/Rust_(language)`. */
function hasOpening(url: string): boolean {
  const close = url.at(-1)!;
  const count = (c: string) => url.split(c).length - 1;
  return count(close === ')' ? '(' : '[') >= count(close);
}

/**
 * Cleans the HTML of an email for the frame: drops scripts, event handlers and code URLs, keeps
 * the email's own styles over the paper, and blocks remote images and styles unless shown.
 */
export function prepareEmailHtml(html: string, showImages: boolean): PreparedEmail {
  const parsed = new DOMParser().parseFromString(html, 'text/html');
  for (const el of Array.from(parsed.querySelectorAll(DROPPED))) el.remove();

  let remote = false;
  for (const el of Array.from(parsed.querySelectorAll('*'))) {
    for (const attr of Array.from(el.attributes)) {
      const name = attr.name.toLowerCase();
      if (name.startsWith('on') || name === 'srcdoc' || name === 'ping') el.removeAttribute(attr.name);
      else if (URL_ATTRIBUTES.includes(name) && isUnsafeUrl(attr.value)) el.removeAttribute(attr.name);
    }
    remote = hideRemote(el, showImages) || remote;
  }
  for (const style of Array.from(parsed.querySelectorAll('style'))) {
    remote = hasRemoteCss(style.textContent) || remote;
  }
  for (const a of Array.from(parsed.querySelectorAll('a[href]'))) {
    a.setAttribute('target', '_blank');
    a.setAttribute('rel', 'noopener noreferrer');
    const href = a.getAttribute('href')!;
    if (!a.hasAttribute('title') && linkTarget(href)) a.setAttribute('title', href);
  }

  const head = parsed.head;
  const policy = parsed.createElement('meta');
  policy.setAttribute('http-equiv', 'Content-Security-Policy');
  policy.setAttribute('content', contentPolicy(showImages));
  const base = parsed.createElement('style');
  base.textContent = BASE_STYLE;
  head.prepend(policy, base);

  const doctype = parsed.doctype ? '<!DOCTYPE html>' : '';
  return { doc: doctype + parsed.documentElement.outerHTML, remote };
}

/**
 * Whether the element loads something from the internet. While images are hidden the remote
 * image addresses are taken out, so the email shows its alt text instead of broken images.
 */
function hideRemote(el: Element, showImages: boolean): boolean {
  let remote = hasRemoteCss(el.getAttribute('style'));
  for (const name of ['src', 'srcset', 'background', 'poster']) {
    const value = el.getAttribute(name);
    const isRemote = name === 'srcset' ? isRemoteSrcset(value) : isRemoteUrl(value);
    if (!isRemote) continue;
    remote = true;
    if (!showImages) el.removeAttribute(name);
  }
  return remote;
}
