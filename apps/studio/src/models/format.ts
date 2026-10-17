// Words and numbers the Models window shows.

/** `8190735360` → `8.2B`, `596000000` → `596M`. */
export function formatParams(n: number | null): string | null {
  if (!n || n <= 0) return null;
  if (n >= 1e9) return `${(n / 1e9).toFixed(n >= 1e11 ? 0 : 1).replace(/\.0$/, '')}B`;
  return `${Math.round(n / 1e6)}M`;
}

/** `262144` → `256K tokens`. */
export function formatContext(tokens: number | null): string | null {
  if (!tokens || tokens <= 0) return null;
  return tokens >= 1024 ? `${Math.round(tokens / 1024)}K tokens` : `${tokens} tokens`;
}

/** How long ago an ISO date was: `today`, `3 days ago`, `2 months ago`. */
export function formatAgo(iso: string | null, now = Date.now()): string | null {
  if (!iso) return null;
  const then = Date.parse(iso);
  if (Number.isNaN(then)) return null;
  const days = Math.floor((now - then) / 86_400_000);
  if (days < 1) return 'today';
  if (days === 1) return 'yesterday';
  if (days < 14) return `${days} days ago`;
  if (days < 60) return `${Math.round(days / 7)} weeks ago`;
  if (days < 365) return `${Math.round(days / 30)} months ago`;
  const years = Math.round(days / 365);
  return years === 1 ? 'a year ago' : `${years} years ago`;
}

/** Licenses as their owners write them: `apache-2.0` → `Apache 2.0`. */
export function formatLicense(license: string | null): string | null {
  if (!license) return null;
  const known: Record<string, string> = {
    'apache-2.0': 'Apache 2.0',
    mit: 'MIT',
    gemma: 'Gemma',
    llama2: 'Llama 2',
    llama3: 'Llama 3',
    'llama3.1': 'Llama 3.1',
    'llama3.2': 'Llama 3.2',
    'llama3.3': 'Llama 3.3',
    llama4: 'Llama 4',
    'cc-by-4.0': 'CC BY 4.0',
    'cc-by-nc-4.0': 'CC BY-NC 4.0',
    'bsd-3-clause': 'BSD 3-Clause',
    'gpl-3.0': 'GPL 3.0',
    openrail: 'OpenRAIL',
    other: 'Other (see the model card)',
  };
  return known[license.toLowerCase()] ?? license;
}
