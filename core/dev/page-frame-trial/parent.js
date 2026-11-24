// Frames /page-frame as the window does (DECISIONS Q15), hands it the probe page, and writes what it reports.
const mode = new URLSearchParams(location.search).get('mode') || 'attr';
const out = document.getElementById('out');
// What the window itself has of WebRTC, beside what the page in its frame could do.
const got = [{ window: { RTCPeerConnection: typeof RTCPeerConnection, webkitRTCPeerConnection: typeof webkitRTCPeerConnection } }];
addEventListener('message', (e) => { got.push(e.data); out.textContent = JSON.stringify({ mode, got }, null, 1); });
fetch('/probe.txt').then((r) => r.text()).then((text) => {
  const f = document.createElement('iframe');
  if (mode === 'attr') f.setAttribute('sandbox', 'allow-scripts');
  f.referrerPolicy = 'no-referrer';
  f.src = '/page-frame';
  let given = false;
  f.onload = () => { if (given) return; given = true; f.contentWindow.postMessage(text, '*'); out.textContent = 'given'; };
  document.body.append(f);
});
