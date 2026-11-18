// Loads the Fishrod engine, compiled to WebAssembly, and talks to it through
// its text command interface (see web/src/lib.rs for the commands). Works in
// browsers, in a Web Worker too, and in Node.
//
// The engine is built for WASI, the WebAssembly system interface, but only
// uses five of its functions; they are provided here.

export async function loadEngine(bytes) {
  let memory;
  const view = () => new DataView(memory.buffer);
  const encoder = new TextEncoder();
  const decoder = new TextDecoder();
  let errorText = "";

  const wasi = {
    // No environment variables.
    environ_sizes_get(countAddress, sizeAddress) {
      view().setUint32(countAddress, 0, true);
      view().setUint32(sizeAddress, 0, true);
      return 0;
    },
    environ_get() {
      return 0;
    },
    // The time in nanoseconds: the monotonic clock (id 1) for measuring how
    // long the engine thinks, the realtime clock (id 0) otherwise.
    clock_time_get(id, precision, timeAddress) {
      const millis = id === 0 ? Date.now() : performance.now();
      view().setBigUint64(timeAddress, BigInt(Math.round(millis * 1e6)), true);
      return 0;
    },
    // Only error messages are ever written; they go to the console.
    fd_write(fd, vectors, count, writtenAddress) {
      let written = 0;
      for (let i = 0; i < count; i++) {
        const address = view().getUint32(vectors + 8 * i, true);
        const length = view().getUint32(vectors + 8 * i + 4, true);
        errorText += decoder.decode(new Uint8Array(memory.buffer, address, length));
        written += length;
      }
      const lines = errorText.split("\n");
      errorText = lines.pop();
      for (const line of lines) console.error(`Fishrod: ${line}`);
      view().setUint32(writtenAddress, written, true);
      return 0;
    },
    proc_exit(code) {
      throw new Error(`Fishrod stopped with exit code ${code}`);
    },
  };

  const { instance } = await WebAssembly.instantiate(bytes, {
    wasi_snapshot_preview1: wasi,
  });
  memory = instance.exports.memory;
  const { fishrod_buffer, fishrod_buffer_size, fishrod_command } = instance.exports;
  const size = fishrod_buffer_size() >>> 0;

  return {
    // Sends one command and returns the engine's answer.
    command(text) {
      const bytes = encoder.encode(text);
      if (bytes.length > size) throw new Error("Fishrod: command too long");
      new Uint8Array(memory.buffer, fishrod_buffer() >>> 0, bytes.length).set(bytes);
      const length = fishrod_command(bytes.length) >>> 0;
      return decoder.decode(new Uint8Array(memory.buffer, fishrod_buffer() >>> 0, length));
    },
  };
}
