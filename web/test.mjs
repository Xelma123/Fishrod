// Checks the WebAssembly engine from Node, through the same loader the page
// uses. Build it first with tools/build-web.sh, then: node web/test.mjs
//
// Given the node count of the native `fishrod bench`, it also checks that
// the WebAssembly build searches exactly the same: node web/test.mjs 162003

import { readFile } from "node:fs/promises";
import { loadEngine } from "./www/fishrod.js";

const engine = await loadEngine(await readFile(new URL("./www/fishrod.wasm", import.meta.url)));
let failures = 0;

function check(name, answer, ok) {
  console.log(`${ok ? "ok  " : "FAIL"} ${name}: ${answer.replaceAll("\n", " | ")}`);
  if (!ok) failures++;
}

const start = engine.command("state");
check("start", start, start.includes("turn w") && start.includes("status ongoing"));
const mate = engine.command("state f2f3 e7e5 g2g4 d8h4");
check("checkmate", mate, mate.includes("status checkmate") && mate.endsWith("san f3 e5 g4 Qh4#"));
const illegal = engine.command("state e2e5");
check("illegal move", illegal, illegal.startsWith("error "));

const reply = engine.command("play 1000 10 0 42 e2e4");
const legal = engine.command("state e2e4").split("\n")[3].split(" ").slice(1);
check("reply to 1.e4", reply, legal.includes(reply.split(" ")[1]));
const mateInOne = engine.command("play 300 10 0 7 f2f3 e7e5 g2g4");
check("mate in one", mateInOne, mateInOne.startsWith("bestmove d8h4 "));
check("new game", engine.command("newgame"), engine.command("newgame") === "ok");

const bench = engine.command("bench");
const expected = process.argv[2];
check(
  expected ? `bench equals the native ${expected}` : "bench",
  bench,
  expected ? bench.startsWith(`${expected} nodes `) : /^\d+ nodes \d+ nps$/.test(bench)
);

process.exit(failures === 0 ? 0 : 1);
