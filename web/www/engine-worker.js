// Runs the engine in a Web Worker, so that the page stays responsive while it
// thinks. Each message { id, command } is answered with { id, answer } or,
// if the engine fails, { id, error }.

import { loadEngine } from "./fishrod.js";

const engine = fetch("fishrod.wasm")
  .then((response) => response.arrayBuffer())
  .then(loadEngine);

onmessage = async (event) => {
  const { id, command } = event.data;
  try {
    const answer = (await engine).command(command);
    postMessage({ id, answer });
  } catch (error) {
    postMessage({ id, error: String(error) });
  }
};
