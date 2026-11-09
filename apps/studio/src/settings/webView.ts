// What the Web & browser page makes of what is typed into it.

/** What the SearXNG field holds as a server's address: null for none, undefined when it is not one. */
export function searxngAddress(text: string): string | null | undefined {
  const t = text.trim();
  if (!t) return null;
  try {
    const url = new URL(/^[a-z][a-z0-9+.-]*:\/\//i.test(t) ? t : `https://${t}`);
    return url.protocol === 'http:' || url.protocol === 'https:' ? url.toString().replace(/\/$/, '') : undefined;
  } catch {
    return undefined;
  }
}
