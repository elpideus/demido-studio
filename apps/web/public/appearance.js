/* Apply saved appearance before first paint: theme, text size, accent. A file of its own, not a script in
   the page: the window allows no script but its own files (the core's content policy). */
try {
  var s = JSON.parse(localStorage.getItem('demido-preview-settings') || '{}')
  var el = document.documentElement
  if (s['appearance.theme'] === 'dark' || s['appearance.theme'] === 'light') el.dataset.theme = s['appearance.theme']
  if (s['appearance.textSize']) el.style.fontSize = s['appearance.textSize'] + '%'
} catch (e) {}
