// Runs on the audio thread: hands the microphone's samples to the page, a block at a time.
// Loaded from the app's own files (the content security policy allows no blob: scripts).

const BLOCK = 4096;

class Capture extends AudioWorkletProcessor {
  constructor() {
    super();
    this.block = new Float32Array(BLOCK);
    this.filled = 0;
    this.port.onmessage = (e) => {
      if (e.data === 'flush') {
        this.port.postMessage(this.block.slice(0, this.filled));
        this.filled = 0;
        this.port.postMessage('flushed');
      }
    };
  }

  process(inputs) {
    const channel = inputs[0] && inputs[0][0];
    if (!channel) return true;
    let at = 0;
    while (at < channel.length) {
      const n = Math.min(channel.length - at, BLOCK - this.filled);
      this.block.set(channel.subarray(at, at + n), this.filled);
      this.filled += n;
      at += n;
      if (this.filled === BLOCK) {
        this.port.postMessage(this.block);
        this.block = new Float32Array(BLOCK);
        this.filled = 0;
      }
    }
    return true;
  }
}

registerProcessor('demido-capture', Capture);
