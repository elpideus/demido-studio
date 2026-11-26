import { StrictMode } from 'react'
import { createRoot } from 'react-dom/client'
import { HashRouter } from 'react-router'
import '@brand/css/demido.css'
import './styles/app.css'
import { App } from './app/App'

// A part of the window that is loaded when first needed (the editor, a chart) can be gone: the core was updated
// while this window stayed open, and its files have new names. The window then loads the new build once,
// instead of showing nothing. Not twice within a minute, so a part that is truly missing does not loop.
window.addEventListener('vite:preloadError', (event) => {
  let last = 0
  try { last = Number(sessionStorage.getItem('demido.reloadedForParts') ?? 0) } catch { /* no storage: reload anyway */ }
  if (Date.now() - last < 60_000) return
  try { sessionStorage.setItem('demido.reloadedForParts', String(Date.now())) } catch { /* as above */ }
  event.preventDefault()
  window.location.reload()
})

createRoot(document.getElementById('root')!).render(
  <StrictMode>
    <HashRouter>
      <App />
    </HashRouter>
  </StrictMode>,
)
