/* Overview: play the logo draw-in once on load and again on request. */
(function () {
  'use strict'
  var mark = document.querySelector('[data-logo-draw]')
  if (!mark) return
  function play() {
    mark.removeAttribute('data-play')
    void mark.getBoundingClientRect() // restart the keyframes
    mark.setAttribute('data-play', '')
  }
  play()
  var btn = document.querySelector('[data-logo-replay]')
  if (btn) btn.addEventListener('click', play)
})()
