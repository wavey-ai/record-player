class VinylAcousticLoopbackCapture extends AudioWorkletProcessor {
  constructor() {
    super();
    this.capacity = 2048;
    this.samples = new Float32Array(this.capacity);
    this.length = 0;
    this.startFrame = 0;
    this.active = true;
    this.port.onmessage = event => {
      if (event.data?.type !== "stop") return;
      this.flush();
      this.active = false;
    };
  }

  flush() {
    if (this.length === 0) return;
    const packet = this.samples.slice(0, this.length);
    this.port.postMessage({
      type: "capture",
      startFrame: this.startFrame,
      samples: packet,
    }, [packet.buffer]);
    this.length = 0;
  }

  process(inputs, outputs) {
    for (const output of outputs[0] || []) output.fill(0);
    const input = inputs[0]?.[0];
    if (input?.length) {
      let sourceOffset = 0;
      while (sourceOffset < input.length) {
        if (this.length === 0) this.startFrame = currentFrame + sourceOffset;
        const count = Math.min(input.length - sourceOffset, this.capacity - this.length);
        this.samples.set(input.subarray(sourceOffset, sourceOffset + count), this.length);
        this.length += count;
        sourceOffset += count;
        if (this.length === this.capacity) this.flush();
      }
    }
    return this.active;
  }
}

registerProcessor("vinyl-acoustic-loopback-capture", VinylAcousticLoopbackCapture);
