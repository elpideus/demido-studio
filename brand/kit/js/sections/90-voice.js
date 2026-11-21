/* Voice: show the approximate token cost of each prompt example. */
(function () {
  'use strict'
  document.querySelectorAll('[data-count-for]').forEach(function (out) {
    var el = document.getElementById(out.dataset.countFor)
    if (!el) return
    var words = el.textContent.trim().split(/\s+/).length
    out.textContent = 'approx. ' + Math.round(words * 1.3) + ' tokens'
  })
})()
