import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';

import '@demido/ui/tokens.css';
import '@demido/ui/base.css';

import { Setup } from './Setup';

createRoot(document.getElementById('root')!).render(
  <StrictMode>
    <Setup />
  </StrictMode>,
);
