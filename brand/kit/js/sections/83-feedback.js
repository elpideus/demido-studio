/* Feedback: open the real dialog (native <dialog>, focus is trapped and Esc closes it). */
(function () {
  'use strict'
  var dialog = document.querySelector('[data-dialog]')
  var open = document.querySelector('[data-dialog-open]')
  if (!dialog || !open || !dialog.showModal) return
  open.addEventListener('click', function () {
    dialog.showModal()
  })
  dialog.addEventListener('click', function (e) {
    if (e.target === dialog) dialog.close() // a click on the scrim cancels
  })
})()
