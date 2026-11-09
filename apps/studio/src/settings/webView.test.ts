import { describe, expect, it } from 'vitest';

import { searxngAddress } from './webView';

describe('searxngAddress', () => {
  it('reads a server address, with or without its scheme', () => {
    expect(searxngAddress('https://searx.example.org/')).toBe('https://searx.example.org');
    expect(searxngAddress(' searx.example.org ')).toBe('https://searx.example.org');
    expect(searxngAddress('http://localhost:8888')).toBe('http://localhost:8888');
    expect(searxngAddress('https://example.org/searx/')).toBe('https://example.org/searx');
  });

  it('takes empty as no server and refuses what is not one', () => {
    expect(searxngAddress('   ')).toBeNull();
    expect(searxngAddress('ftp://example.org')).toBeUndefined();
    expect(searxngAddress('http://')).toBeUndefined();
  });
});
