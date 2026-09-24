// Identifies Demido Studio on outgoing requests made through the global fetch.
//
// dukascopy-node calls fetch() without headers, and Dukascopy's public feed answers Node's
// default user agent with "429 Too Many Requests" regardless of the request rate. Naming the
// app is enough for it to answer normally.

export const USER_AGENT = 'DemidoStudio/0.1 (+https://github.com/elpideus/demido-studio)';

const nativeFetch = globalThis.fetch;

globalThis.fetch = (input: string | URL | Request, init?: RequestInit): Promise<Response> => {
  const headers = new Headers(init?.headers ?? (input instanceof Request ? input.headers : undefined));
  if (!headers.has('user-agent')) headers.set('user-agent', USER_AGENT);
  return nativeFetch(input, { ...init, headers });
};
