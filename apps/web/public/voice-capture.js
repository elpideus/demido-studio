/* The microphone, for voice mode (DECISIONS V7): the browser's sound, cancelled for echo by the browser itself,
   brought down to 16 kHz mono and handed over 20 ms at a time as 16-bit samples, with how loud it was. Served
   from Demido's own address, as the window's policy asks of every script. */
class Capture extends AudioWorkletProcessor {
  constructor() {
    super()
    this.ratio = sampleRate / 16000
    this.phase = 0
    this.sum = 0
    this.count = 0
    this.frame = new Int16Array(320)
    this.filled = 0
    this.power = 0
  }

  process(inputs) {
    const input = inputs[0] && inputs[0][0]
    if (!input) return true
    for (let i = 0; i < input.length; i++) {
      // Each 16 kHz sample is the mean of the samples it stands for: a plain low-pass before taking fewer.
      this.sum += input[i]
      this.count += 1
      this.phase += 1
      if (this.phase >= this.ratio) {
        this.phase -= this.ratio
        const v = Math.max(-1, Math.min(1, this.sum / this.count))
        this.sum = 0
        this.count = 0
        this.frame[this.filled++] = v < 0 ? v * 32768 : v * 32767
        this.power += v * v
        if (this.filled === this.frame.length) {
          const level = Math.min(1, Math.sqrt(this.power / this.frame.length) * 6)
          this.port.postMessage({ pcm: this.frame.buffer, level }, [this.frame.buffer])
          this.frame = new Int16Array(320)
          this.filled = 0
          this.power = 0
        }
      }
    }
    return true
  }
}

registerProcessor('demido-capture', Capture)
