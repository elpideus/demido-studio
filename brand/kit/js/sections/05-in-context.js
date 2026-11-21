/* In the app: flip the composed window between dark and light without touching the page theme. */
(function () {
  'use strict'
  var frame = document.querySelector('[data-kit-app]')
  if (!frame) return
  var radios = document.querySelectorAll('input[name="kit-app-theme"]')
  function sync() {
    radios.forEach(function (r) {
      if (r.checked) frame.dataset.theme = r.value
    })
  }
  radios.forEach(function (r) {
    r.addEventListener('change', sync)
  })
  // Start in whatever theme the page is showing.
  function follow() {
    var page = document.documentElement.dataset.theme || (window.matchMedia('(prefers-color-scheme: light)').matches ? 'light' : 'dark')
    radios.forEach(function (r) {
      r.checked = r.value === page
    })
    sync()
  }
  follow()
  document.addEventListener('kit:theme', follow)
})()
