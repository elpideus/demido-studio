// Where the addresses in a Hugging Face model card point. Cards are written to be read on the Hub,
// so their pictures and links are often relative to the repository.

const HUB = 'https://huggingface.co';

/**
 * The address to load (an image) or open (a link) for `url` as `repo`'s card writes it. A page
 * link to a picture on the Hub (`/blob/`) becomes the file itself (`/resolve/`), which is what an
 * `<img>` can show. `#section` stays as it is, for the card to scroll to. Anything that is not
 * the web or mail is dropped.
 */
export function resolveCardUrl(url: string | undefined, repo: string, kind: 'image' | 'link'): string | null {
  const raw = url?.trim();
  if (!raw) return null;
  if (raw.startsWith('#')) return kind === 'link' ? raw : null;
  let absolute: string;
  if (/^https?:\/\//i.test(raw)) absolute = raw;
  else if (raw.startsWith('//')) absolute = `https:${raw}`;
  else if (/^mailto:/i.test(raw)) return kind === 'link' ? raw : null;
  else if (/^[a-z][a-z0-9+.-]*:/i.test(raw)) return null;
  else if (raw.startsWith('/')) absolute = `${HUB}${raw}`;
  else {
    const path = raw.replace(/^\.\//, '');
    absolute = `${HUB}/${repo}/${kind === 'image' ? 'resolve' : 'blob'}/main/${path}`;
  }
  if (kind === 'image') {
    absolute = absolute.replace(
      /^(https:\/\/huggingface\.co\/(?:datasets\/|spaces\/)?[^/]+\/[^/]+)\/blob\//,
      '$1/resolve/',
    );
  }
  return absolute;
}

/** Every address in a `srcset`, resolved; the widths and densities after them kept. */
export function resolveSrcSet(srcSet: string | undefined, repo: string): string | undefined {
  if (!srcSet) return undefined;
  const parts = srcSet
    .split(',')
    .map((part) => {
      const [url, ...descriptor] = part.trim().split(/\s+/);
      const resolved = resolveCardUrl(url, repo, 'image');
      return resolved ? [resolved, ...descriptor].join(' ') : null;
    })
    .filter((p): p is string => p !== null);
  return parts.length ? parts.join(', ') : undefined;
}

/** A heading's anchor as the Hub makes it: `Quick start (llama.cpp)` → `quick-start-llamacpp`. */
export function slug(text: string): string {
  return text
    .trim()
    .toLowerCase()
    .replace(/[^\p{L}\p{N}\s-]/gu, '')
    .replace(/\s+/g, '-');
}
