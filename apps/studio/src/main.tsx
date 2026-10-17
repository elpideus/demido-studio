import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';

import '@demido/ui/fonts.css';
import '@demido/ui/tokens.css';
import '@demido/ui/base.css';
import './styles/app.css';

import { App } from './App';
import { installDevConsole } from './lib/devConsole';

if (import.meta.env.DEV) installDevConsole();

createRoot(document.getElementById('root')!).render(
  <StrictMode>
    <App />
  </StrictMode>,
);
