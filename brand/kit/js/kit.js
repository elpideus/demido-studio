/* Brand kit page behaviour: theme choice, current-section highlight, click-to-copy token names.
   Classic script (no modules) so the page also works when opened straight from disk. */
(function () {
  'use strict'
  var root = document.documentElement
  var KEY = 'demido-kit-theme'

  // ---------- theme ----------
  var choices = document.querySelectorAll('[data-theme-choice]')
  function applyTheme(choice) {
    if (choice === 'dark' || choice === 'light') root.dataset.theme = choice
    else delete root.dataset.theme
    choices.forEach(function (b) {
      b.setAttribute('aria-checked', String(b.dataset.themeChoice === choice))
    })
    document.dispatchEvent(new CustomEvent('kit:theme', { detail: choice }))
  }
  var saved = 'system'
  try {
    saved = new URLSearchParams(location.search).get('theme') || localStorage.getItem(KEY) || 'system'
  } catch (e) {}
  applyTheme(saved)
  choices.forEach(function (b) {
    b.addEventListener('click', function () {
      var c = b.dataset.themeChoice
      try {
        if (c === 'system') localStorage.removeItem(KEY)
        else localStorage.setItem(KEY, c)
      } catch (e) {}
      applyTheme(c)
    })
  })

  // ---------- current section ----------
  var links = Array.prototype.slice.call(document.querySelectorAll('.kit-nav a'))
  var byId = {}
  links.forEach(function (a) {
    byId[a.getAttribute('href').slice(1)] = a
  })
  var sections = Array.prototype.slice.call(document.querySelectorAll('.kit-section'))
  function setCurrent(id) {
    links.forEach(function (a) {
      if (a === byId[id]) a.setAttribute('aria-current', 'true')
      else a.removeAttribute('aria-current')
    })
  }
  if ('IntersectionObserver' in window && sections.length) {
    var visible = new Set()
    var io = new IntersectionObserver(
      function (entries) {
        entries.forEach(function (e) {
          if (e.isIntersecting) visible.add(e.target)
          else visible.delete(e.target)
        })
        var first = sections.find(function (s) {
          return visible.has(s)
        })
        if (first) setCurrent(first.id)
      },
      { rootMargin: '-20% 0px -65% 0px' },
    )
    sections.forEach(function (s) {
      io.observe(s)
    })
  }

  // ---------- copy token names ----------
  var toast = document.querySelector('.kit-toast')
  var toastTimer
  function say(text) {
    if (!toast) return
    toast.textContent = text
    toast.hidden = false
    toast.dataset.state = 'closed'
    requestAnimationFrame(function () {
      toast.dataset.state = 'open'
    })
    clearTimeout(toastTimer)
    toastTimer = setTimeout(function () {
      toast.dataset.state = 'closed'
      setTimeout(function () {
        toast.hidden = true
      }, 220)
    }, 1400)
  }
  document.addEventListener('click', function (e) {
    var chip = e.target.closest('.kit-token')
    if (!chip) return
    var text = chip.dataset.copy || chip.textContent.trim()
    var done = function () {
      say('Copied ' + text)
    }
    if (navigator.clipboard && navigator.clipboard.writeText) navigator.clipboard.writeText(text).then(done, done)
    else done()
  })

  window.DemidoKit = { say: say }
})()
