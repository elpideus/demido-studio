// Resilient host name resolution for every connection the service makes (axios, websockets,
// fetch).
//
// On some Windows setups the system resolver stalls for about ten seconds on a first lookup of
// a CDN host name and then reports ENOTFOUND, while querying the DNS server directly answers at
// once. So: ask the system first, and if it has not answered within a moment (or fails), ask the
// DNS servers directly; whichever answers first wins.

import dns, { type LookupAddress, type LookupOptions } from 'node:dns';

const systemLookup = dns.lookup.bind(dns);
const FALLBACK_AFTER_MS = 1500;

type Callback = (err: NodeJS.ErrnoException | null, address: string | LookupAddress[], family?: number) => void;

function direct(hostname: string, family: number | undefined): Promise<LookupAddress[]> {
  const v4 = () => dns.promises.resolve4(hostname).then((a) => a.map((address) => ({ address, family: 4 })));
  const v6 = () => dns.promises.resolve6(hostname).then((a) => a.map((address) => ({ address, family: 6 })));
  if (family === 4) return v4();
  if (family === 6) return v6();
  // IPv6 is only asked for when IPv4 has nothing, so no query is left without a handler.
  return v4().catch(() => v6());
}

function resilientLookup(hostname: string, options: unknown, callback?: unknown): void {
  let opts: LookupOptions = {};
  let cb = callback as Callback;
  if (typeof options === 'function') cb = options as Callback;
  else if (typeof options === 'number') opts = { family: options };
  else if (options && typeof options === 'object') opts = options as LookupOptions;

  let settled = false;
  const finish = (err: NodeJS.ErrnoException | null, list: LookupAddress[]) => {
    if (settled) return;
    settled = true;
    clearTimeout(timer);
    if (err || list.length === 0) {
      cb(err ?? Object.assign(new Error(`getaddrinfo ENOTFOUND ${hostname}`), { code: 'ENOTFOUND' }), '', 0);
    } else if (opts.all) {
      cb(null, list);
    } else {
      cb(null, list[0]!.address, list[0]!.family);
    }
  };
  let systemError: NodeJS.ErrnoException | null = null;
  const fallback = () => {
    direct(hostname, typeof opts.family === 'number' ? opts.family : undefined).then(
      (list) => finish(null, list),
      () => {
        if (systemError) finish(systemError, []);
      },
    );
  };
  const timer = setTimeout(fallback, FALLBACK_AFTER_MS);
  systemLookup(hostname, { ...opts, all: true }, (err, addresses) => {
    if (!err) {
      finish(null, addresses as LookupAddress[]);
      return;
    }
    systemError = err;
    if (['ENOTFOUND', 'EAI_AGAIN', 'ETIMEOUT', 'ESERVFAIL', 'ECONNREFUSED'].includes(err.code ?? '')) {
      clearTimeout(timer);
      fallback();
    } else {
      finish(err, []);
    }
  });
}

// Node's net module reads dns.lookup at connection time, so replacing it covers every client.
(dns as unknown as { lookup: typeof resilientLookup }).lookup = resilientLookup;
