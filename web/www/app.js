// Fishrod web: the board, the game, and the engine in a Web Worker.
//
// The engine decides everything about the rules: after every move the page
// asks it for the position (legal moves, check, how the game stands, the
// moves in algebraic notation) and draws that. The page itself only keeps
// the list of moves played, in UCI notation, which is also what it saves.

const THINK_MS = 3000;
const VARIETY_CP = 10;
const STORAGE_KEY = "fishrod.game.v1";

// The engine's strength setting (UCI_Elo) for each level; 0 is full
// strength. The numbers are rough until they are measured.
const LEVELS = [
  { id: "beginner", name: "Başlangıç", elo: 800, text: "Satrancı yeni öğrenenler için. Sık sık taş kaybeder." },
  { id: "easy", name: "Kolay", elo: 1200, text: "Basit tehditleri görür ama çok hata yapar." },
  { id: "medium", name: "Orta", elo: 1600, text: "Dikkatli oynar; zaman zaman hata yapar." },
  { id: "hard", name: "Zor", elo: 1900, text: "Güçlü bir rakip; nadiren hata yapar." },
  { id: "max", name: "Tam güç", elo: 0, text: "Motorun bütün gücü." },
];

const PIECE_NAMES = { K: "şah", Q: "vezir", R: "kale", B: "fil", N: "at", P: "piyon" };
const PIECE_VALUES = { P: 1, N: 3, B: 3, R: 5, Q: 9, K: 0 };
const FILES = "abcdefgh";

class Cancelled extends Error {}

// The engine, running in a Web Worker. A search cannot be interrupted, so
// cancelling one replaces the worker with a fresh one.
class Engine {
  constructor() {
    this.start();
  }

  start() {
    this.worker = new Worker("engine-worker.js", { type: "module" });
    this.nextId = 0;
    this.pending = new Map();
    this.worker.onmessage = ({ data }) => {
      const request = this.pending.get(data.id);
      if (!request) return;
      this.pending.delete(data.id);
      if (data.error) request.reject(new Error(data.error));
      else request.resolve(data.answer);
    };
    this.worker.onerror = (event) => {
      event.preventDefault();
      this.failAll(new Error("Motor başlatılamadı."));
    };
  }

  ask(command) {
    return new Promise((resolve, reject) => {
      const id = this.nextId++;
      this.pending.set(id, { resolve, reject });
      this.worker.postMessage({ id, command });
    });
  }

  restart() {
    this.worker.terminate();
    this.failAll(new Cancelled());
    this.start();
  }

  failAll(error) {
    for (const request of this.pending.values()) request.reject(error);
    this.pending.clear();
  }
}

const engine = new Engine();

const game = {
  moves: [],
  san: [],
  board: new Array(64).fill(null),
  turn: "w",
  legal: [],
  status: "ongoing",
  check: false,
  player: "w",
  level: "medium",
  orientation: "w",
  resigned: false,
  thinking: false,
  ready: false,
  error: null,
  selected: null,
};

// Raised whenever what the page waits for no longer matters: a new game, a
// move taken back. Answers that arrive for an older epoch are dropped.
let epoch = 0;

const $ = (id) => document.getElementById(id);
const boardEl = $("board");
const squares = new Array(64);

// Squares and moves

const squareName = (sq) => FILES[sq % 8] + (Math.floor(sq / 8) + 1);
const squareIndex = (name) => FILES.indexOf(name[0]) + 8 * (Number(name[1]) - 1);
const opposite = (color) => (color === "w" ? "b" : "w");
const colorName = (color) => (color === "w" ? "Beyaz" : "Siyah");
const level = () => LEVELS.find((l) => l.id === game.level) ?? LEVELS[2];

function movesFrom(sq) {
  const name = squareName(sq);
  return game.legal.filter((move) => move.startsWith(name));
}

function movesBetween(from, to) {
  const prefix = squareName(from) + squareName(to);
  return game.legal.filter((move) => move.startsWith(prefix));
}

function parseBoard(fen) {
  const board = new Array(64).fill(null);
  const rows = fen.split(" ")[0].split("/");
  rows.forEach((row, i) => {
    const rank = 7 - i;
    let file = 0;
    for (const char of row) {
      if (/\d/.test(char)) {
        file += Number(char);
      } else {
        const color = char === char.toUpperCase() ? "w" : "b";
        board[rank * 8 + file] = color + char.toUpperCase();
        file++;
      }
    }
  });
  return board;
}

function parseState(answer) {
  if (answer.startsWith("error ")) throw new Error(answer.slice(6));
  const fields = {};
  for (const line of answer.split("\n")) {
    const space = line.indexOf(" ");
    fields[line.slice(0, space)] = line.slice(space + 1);
  }
  return {
    board: parseBoard(fields.fen),
    turn: fields.turn,
    check: fields.check === "1",
    legal: fields.legal ? fields.legal.split(" ") : [],
    status: fields.status,
    san: fields.san ? fields.san.split(" ") : [],
  };
}

// The game

const isOver = () => game.resigned || game.status !== "ongoing";
const playersTurn = () => game.turn === game.player;
const canMove = () => game.ready && !game.thinking && !isOver() && playersTurn();
const playerMoveCount = () =>
  game.player === "w" ? Math.ceil(game.moves.length / 2) : Math.floor(game.moves.length / 2);

async function refresh() {
  const token = epoch;
  const answer = await engine.ask("state " + game.moves.join(" "));
  if (token !== epoch) throw new Cancelled();
  Object.assign(game, parseState(answer));
}

// After the moves have changed: shows the new position, saves the game,
// and lets the engine move when it is its turn.
async function advance(animated) {
  game.selected = null;
  await refresh();
  render(animated);
  save();
  if (isOver()) {
    showResult();
  } else if (!playersTurn()) {
    await engineMove();
  }
}

async function engineMove() {
  const token = epoch;
  game.thinking = true;
  render();
  const seed = Math.floor(Math.random() * 2 ** 52);
  let answer;
  try {
    answer = await engine.ask(
      `play ${THINK_MS} ${VARIETY_CP} ${level().elo} ${seed} ${game.moves.join(" ")}`
    );
  } finally {
    if (token === epoch) game.thinking = false;
  }
  if (token !== epoch) throw new Cancelled();
  const move = answer.split(" ")[1];
  if (!answer.startsWith("bestmove ") || move === "0000") throw new Error(answer);
  game.moves.push(move);
  await advance(move);
}

function run(task) {
  task().catch((error) => {
    if (error instanceof Cancelled) return;
    console.error(error);
    game.error = "Bir hata oluştu. Sayfayı yenileyip tekrar dene.";
    game.thinking = false;
    render();
  });
}

function playMove(move, animated) {
  game.moves.push(move);
  run(() => advance(animated ? move : null));
}

function cancelEngine() {
  epoch++;
  if (game.thinking) {
    engine.restart();
    game.thinking = false;
  }
}

function newGame(color, levelId) {
  cancelEngine();
  engine.ask("newgame").catch(() => {});
  game.moves = [];
  game.player = color === "random" ? (Math.random() < 0.5 ? "w" : "b") : color;
  game.level = levelId;
  game.orientation = game.player;
  game.resigned = false;
  game.error = null;
  buildBoard();
  run(() => advance(null));
}

function takeBack() {
  if (playerMoveCount() === 0) return;
  cancelEngine();
  game.resigned = false;
  // Back to the player's turn, before the player's last move.
  do {
    game.moves.pop();
  } while (game.moves.length > 0 && (game.moves.length % 2 === 0 ? "w" : "b") !== game.player);
  run(() => advance(null));
}

function resign() {
  cancelEngine();
  game.resigned = true;
  save();
  render();
  showResult();
}

// The result of a finished game: a title, a sentence and the PGN result.
function result() {
  const won = (color) => (color === "w" ? "1-0" : "0-1");
  if (game.resigned) {
    return { title: "Teslim oldun", text: "Fishrod kazandı.", pgn: won(opposite(game.player)) };
  }
  switch (game.status) {
    case "checkmate": {
      const winner = opposite(game.turn);
      return winner === game.player
        ? { title: "Mat ettin", text: "Tebrikler, Fishrod'u yendin.", pgn: won(winner) }
        : { title: "Mat oldun", text: "Fishrod kazandı.", pgn: won(winner) };
    }
    case "stalemate":
      return { title: "Pat", text: "Sırası gelen taraf şah altında değil ama oynayacak hamlesi yok. Oyun berabere.", pgn: "1/2-1/2" };
    case "fifty":
      return { title: "50 hamle kuralı", text: "Son 50 hamlede taş alınmadı, piyon oynanmadı. Oyun berabere.", pgn: "1/2-1/2" };
    case "repetition":
      return { title: "Üç kez tekrar", text: "Aynı pozisyon üçüncü kez oluştu. Oyun berabere.", pgn: "1/2-1/2" };
    case "material":
      return { title: "Yetersiz materyal", text: "İki tarafın da mat edecek kadar taşı kalmadı. Oyun berabere.", pgn: "1/2-1/2" };
    default:
      return null;
  }
}

function pgn() {
  const outcome = result();
  const date = new Date();
  const pad = (n) => String(n).padStart(2, "0");
  const name = (color) => (color === game.player ? "Sen" : `Fishrod (${level().name})`);
  const headers = [
    ["Event", "Fishrod'a karşı"],
    ["Site", location.host || "Fishrod"],
    ["Date", `${date.getFullYear()}.${pad(date.getMonth() + 1)}.${pad(date.getDate())}`],
    ["White", name("w")],
    ["Black", name("b")],
    ["Result", outcome ? outcome.pgn : "*"],
  ];
  const tokens = [];
  game.san.forEach((san, i) => {
    if (i % 2 === 0) tokens.push(`${i / 2 + 1}.`);
    tokens.push(san);
  });
  tokens.push(outcome ? outcome.pgn : "*");
  const lines = [];
  let line = "";
  for (const token of tokens) {
    if (line && line.length + token.length + 1 > 80) {
      lines.push(line);
      line = token;
    } else {
      line = line ? `${line} ${token}` : token;
    }
  }
  lines.push(line);
  return headers.map(([key, value]) => `[${key} "${value}"]`).join("\n") + "\n\n" + lines.join("\n") + "\n";
}

async function copyPgn() {
  const text = pgn();
  try {
    await navigator.clipboard.writeText(text);
  } catch {
    const area = document.createElement("textarea");
    area.value = text;
    area.style.position = "fixed";
    area.style.opacity = "0";
    document.body.append(area);
    area.select();
    document.execCommand("copy");
    area.remove();
  }
  toast("PGN panoya kopyalandı.");
}

// Saving: the game survives closing the page.

function save() {
  try {
    localStorage.setItem(
      STORAGE_KEY,
      JSON.stringify({
        moves: game.moves,
        player: game.player,
        level: game.level,
        orientation: game.orientation,
        resigned: game.resigned,
      })
    );
  } catch {
    // Private browsing or full storage: the game just will not be kept.
  }
}

function load() {
  try {
    const saved = JSON.parse(localStorage.getItem(STORAGE_KEY));
    if (!saved || !Array.isArray(saved.moves)) return;
    game.moves = saved.moves.filter((move) => /^[a-h][1-8][a-h][1-8][qrbn]?$/.test(move));
    game.player = saved.player === "b" ? "b" : "w";
    game.level = LEVELS.some((l) => l.id === saved.level) ? saved.level : "medium";
    game.orientation = saved.orientation === "b" ? "b" : "w";
    game.resigned = saved.resigned === true;
  } catch {
    // Nothing usable saved: start fresh.
  }
}

// Drawing

function buildBoard() {
  boardEl.replaceChildren();
  const whiteBelow = game.orientation === "w";
  for (let row = 0; row < 8; row++) {
    for (let col = 0; col < 8; col++) {
      const file = whiteBelow ? col : 7 - col;
      const rank = whiteBelow ? 7 - row : row;
      const sq = rank * 8 + file;
      const el = document.createElement("div");
      el.className = (file + rank) % 2 === 0 ? "square dark" : "square";
      el.setAttribute("role", "gridcell");
      if (col === 0) el.append(coordinate("coord-rank", rank + 1));
      if (row === 7) el.append(coordinate("coord-file", FILES[file]));
      squares[sq] = el;
      boardEl.append(el);
    }
  }
}

function coordinate(className, text) {
  const span = document.createElement("span");
  span.className = `coord ${className}`;
  span.textContent = text;
  return span;
}

function render(animated) {
  renderBoard(animated);
  renderPlayers();
  renderMoves();
  renderStatus();
  renderButtons();
}

function renderBoard(animated) {
  const last = game.moves.at(-1);
  const lastSquares = last ? [squareIndex(last.slice(0, 2)), squareIndex(last.slice(2, 4))] : [];
  const targets = new Set(
    game.selected === null ? [] : movesFrom(game.selected).map((move) => squareIndex(move.slice(2, 4)))
  );
  const king = game.board.indexOf(game.turn + "K");

  for (let sq = 0; sq < 64; sq++) {
    const el = squares[sq];
    const piece = game.board[sq];
    el.classList.toggle("last", lastSquares.includes(sq));
    el.classList.toggle("selected", sq === game.selected);
    el.classList.toggle("check", game.check && sq === king);
    el.classList.toggle("target", targets.has(sq));
    el.classList.toggle("occupied", piece !== null);
    el.setAttribute(
      "aria-label",
      piece ? `${squareName(sq)}, ${colorName(piece[0]).toLowerCase()} ${PIECE_NAMES[piece[1]]}` : squareName(sq)
    );

    let img = el.querySelector(".piece");
    if (!piece) {
      img?.remove();
      continue;
    }
    if (!img) {
      img = document.createElement("img");
      img.className = "piece";
      img.alt = "";
      img.draggable = false;
      el.append(img);
    }
    const src = `pieces/${piece}.svg`;
    if (img.getAttribute("src") !== src) img.setAttribute("src", src);
  }

  if (animated) slide(squareIndex(animated.slice(0, 2)), squareIndex(animated.slice(2, 4)));
}

// Slides the piece that has just arrived on `to` from `from`.
function slide(from, to) {
  const img = squares[to].querySelector(".piece");
  if (!img || window.matchMedia("(prefers-reduced-motion: reduce)").matches) return;
  const a = squares[from].getBoundingClientRect();
  const b = squares[to].getBoundingClientRect();
  img.classList.remove("moving");
  img.style.transform = `translate(${a.left - b.left}px, ${a.top - b.top}px)`;
  img.getBoundingClientRect(); // apply the start position before moving on
  img.classList.add("moving");
  img.style.transform = "";
  img.addEventListener("transitionend", () => img.classList.remove("moving"), { once: true });
}

function renderPlayers() {
  const counts = { w: {}, b: {} };
  let material = { w: 0, b: 0 };
  for (const piece of game.board) {
    if (!piece) continue;
    counts[piece[0]][piece[1]] = (counts[piece[0]][piece[1]] ?? 0) + 1;
    material[piece[0]] += PIECE_VALUES[piece[1]];
  }
  const sides = { "player-bottom": game.orientation, "player-top": opposite(game.orientation) };
  for (const [id, color] of Object.entries(sides)) {
    const el = $(id);
    const isPlayer = color === game.player;
    el.querySelector(".name").textContent = isPlayer ? "Sen" : "Fishrod";
    el.querySelector(".tag").textContent = isPlayer ? colorName(color) : `${colorName(color)} · ${level().name}`;
    el.querySelector(".thinking").hidden = isPlayer || !game.thinking;

    // The pieces this side has more of than the opponent, shown as the
    // opponent's pieces it is ahead by, and its material lead. Counting the
    // difference rather than what is missing keeps promotions right.
    const captured = el.querySelector(".captured");
    captured.replaceChildren();
    const them = opposite(color);
    for (const type of ["Q", "R", "B", "N", "P"]) {
      const surplus = Math.max(0, (counts[color][type] ?? 0) - (counts[them][type] ?? 0));
      for (let i = 0; i < surplus; i++) {
        const img = document.createElement("img");
        img.src = `pieces/${them}${type}.svg`;
        img.alt = "";
        captured.append(img);
      }
    }
    const lead = material[color] - material[them];
    if (lead > 0) {
      const span = document.createElement("span");
      span.className = "advantage";
      span.textContent = `+${lead}`;
      captured.append(span);
    }
  }
}

function renderMoves() {
  const list = $("moves");
  list.replaceChildren();
  if (game.san.length === 0) {
    const empty = document.createElement("li");
    empty.className = "moves-empty";
    empty.textContent = game.player === "w" ? "İlk hamle senin." : "İlk hamle Fishrod'un.";
    list.append(empty);
    return;
  }
  for (let i = 0; i < game.san.length; i += 2) {
    const row = document.createElement("li");
    const number = document.createElement("span");
    number.className = "number";
    number.textContent = `${i / 2 + 1}.`;
    row.append(number);
    for (const j of [i, i + 1]) {
      const move = document.createElement("span");
      move.className = j === game.san.length - 1 ? "move current" : "move";
      move.textContent = game.san[j] ?? "";
      row.append(move);
    }
    list.append(row);
  }
  list.scrollTop = list.scrollHeight;
}

function renderStatus() {
  const outcome = isOver() ? result() : null;
  let text;
  if (game.error) text = game.error;
  else if (!game.ready) text = "Motor yükleniyor…";
  else if (outcome) text = `${outcome.title} · ${outcome.pgn}`;
  else if (game.thinking) text = "Fishrod düşünüyor…";
  else if (playersTurn()) text = game.check ? "Şah! Sıra sende." : "Sıra sende.";
  else text = "Fishrod'un sırası.";
  $("status").textContent = text;
}

function renderButtons() {
  $("take-back").disabled = !game.ready || playerMoveCount() === 0;
  $("resign").disabled = !game.ready || isOver();
  $("copy-pgn").disabled = game.moves.length === 0;
}

// Moving pieces: tap a piece and then a square, or drag it.

let drag = null;

function squareAt(x, y) {
  const rect = boardEl.getBoundingClientRect();
  if (x < rect.left || x >= rect.right || y < rect.top || y >= rect.bottom) return null;
  const col = Math.floor(((x - rect.left) / rect.width) * 8);
  const row = Math.floor(((y - rect.top) / rect.height) * 8);
  const whiteBelow = game.orientation === "w";
  const file = whiteBelow ? col : 7 - col;
  const rank = whiteBelow ? 7 - row : row;
  return rank * 8 + file;
}

function tryMove(from, to, animated) {
  const moves = movesBetween(from, to);
  if (moves.length === 0) return false;
  if (moves.length > 1) {
    showPromotion(from, to);
  } else {
    game.selected = null;
    playMove(moves[0], animated);
  }
  return true;
}

function onPointerDown(event) {
  if (!event.isPrimary || event.button > 0) return;
  hidePromotion();
  const sq = squareAt(event.clientX, event.clientY);
  if (sq === null || !canMove()) return;
  event.preventDefault();

  if (game.selected !== null && game.selected !== sq && tryMove(game.selected, sq, true)) return;

  const piece = game.board[sq];
  if (piece && piece[0] === game.player) {
    const wasSelected = game.selected === sq;
    game.selected = sq;
    renderBoard();
    drag = { from: sq, x: event.clientX, y: event.clientY, id: event.pointerId, active: false, wasSelected };
    try {
      boardEl.setPointerCapture(event.pointerId);
    } catch {
      // The pointer is already gone; the drag simply ends on the next event.
    }
  } else if (game.selected !== null) {
    game.selected = null;
    renderBoard();
  }
}

function onPointerMove(event) {
  if (!drag || event.pointerId !== drag.id) return;
  if (!drag.active) {
    if (Math.hypot(event.clientX - drag.x, event.clientY - drag.y) < 5) return;
    drag.active = true;
    const img = squares[drag.from].querySelector(".piece");
    const size = squares[drag.from].getBoundingClientRect().width;
    drag.ghost = img.cloneNode();
    drag.ghost.className = "ghost";
    drag.ghost.style.width = drag.ghost.style.height = `${size}px`;
    document.body.append(drag.ghost);
    img.classList.add("lifted");
  }
  drag.ghost.style.left = `${event.clientX}px`;
  drag.ghost.style.top = `${event.clientY}px`;
  const over = squareAt(event.clientX, event.clientY);
  if (drag.hover !== over) {
    if (drag.hover != null) squares[drag.hover].classList.remove("hover");
    if (over != null) squares[over].classList.add("hover");
    drag.hover = over;
  }
}

function endDrag(event, dropped) {
  if (!drag || event.pointerId !== drag.id) return;
  const current = drag;
  drag = null;
  if (current.hover != null) squares[current.hover].classList.remove("hover");
  squares[current.from].querySelector(".piece")?.classList.remove("lifted");
  current.ghost?.remove();
  if (!dropped) return;

  if (current.active) {
    const to = squareAt(event.clientX, event.clientY);
    if (to !== null && to !== current.from) tryMove(current.from, to, false);
  } else if (current.wasSelected) {
    // A second tap on the selected piece puts it back down.
    game.selected = null;
    renderBoard();
  }
}

// Promotion: a column of four pieces over the promotion square.

function showPromotion(from, to) {
  const chooser = $("promotion");
  const whiteBelow = game.orientation === "w";
  const file = to % 8;
  const rank = Math.floor(to / 8);
  const col = whiteBelow ? file : 7 - file;
  const atTop = (whiteBelow ? 7 - rank : rank) === 0;
  chooser.style.left = `${col * 12.5}%`;
  chooser.style.top = atTop ? "0" : "auto";
  chooser.style.bottom = atTop ? "auto" : "0";
  chooser.replaceChildren();
  const order = atTop ? ["q", "n", "r", "b"] : ["b", "r", "n", "q"];
  for (const letter of order) {
    const button = document.createElement("button");
    button.type = "button";
    const type = letter.toUpperCase();
    button.setAttribute("aria-label", PIECE_NAMES[type]);
    const img = document.createElement("img");
    img.src = `pieces/${game.player}${type}.svg`;
    img.alt = "";
    button.append(img);
    button.addEventListener("click", () => {
      hidePromotion();
      game.selected = null;
      playMove(squareName(from) + squareName(to) + letter, true);
    });
    chooser.append(button);
  }
  chooser.hidden = false;
  chooser.querySelector("button").focus();
}

function hidePromotion() {
  $("promotion").hidden = true;
}

// Dialogs and messages

function openNewGame() {
  const form = $("new-game-form");
  form.elements.color.value = game.player;
  form.elements.level.value = game.level;
  $("new-game-dialog").showModal();
}

function showResult() {
  const outcome = result();
  if (!outcome) return;
  $("result-title").textContent = outcome.title;
  $("result-text").textContent = outcome.text;
  const dialog = $("result-dialog");
  if (!dialog.open) dialog.showModal();
}

let toastTimer;
function toast(text) {
  const el = $("toast");
  el.textContent = text;
  el.hidden = false;
  clearTimeout(toastTimer);
  toastTimer = setTimeout(() => (el.hidden = true), 2200);
}

function buildLevels() {
  const container = $("levels");
  for (const l of LEVELS) {
    const label = document.createElement("label");
    label.innerHTML = `<input type="radio" name="level" value="${l.id}"><span><strong></strong><small></small></span>`;
    label.querySelector("strong").textContent = l.name;
    label.querySelector("small").textContent = l.text;
    container.append(label);
  }
}

// Resigning takes a second tap, so a stray tap cannot end the game.
let resignTimer;
function onResign() {
  const button = $("resign");
  if (button.classList.contains("confirm")) {
    clearTimeout(resignTimer);
    button.classList.remove("confirm");
    button.textContent = "Teslim ol";
    resign();
    return;
  }
  button.classList.add("confirm");
  button.textContent = "Emin misin?";
  resignTimer = setTimeout(() => {
    button.classList.remove("confirm");
    button.textContent = "Teslim ol";
  }, 3000);
}

// Start

function bind() {
  boardEl.addEventListener("pointerdown", onPointerDown);
  boardEl.addEventListener("pointermove", onPointerMove);
  boardEl.addEventListener("pointerup", (event) => endDrag(event, true));
  boardEl.addEventListener("pointercancel", (event) => endDrag(event, false));
  boardEl.addEventListener("contextmenu", (event) => event.preventDefault());
  document.addEventListener("keydown", (event) => {
    if (event.key === "Escape") hidePromotion();
  });

  $("new-game").addEventListener("click", openNewGame);
  $("new-game-top").addEventListener("click", openNewGame);
  $("take-back").addEventListener("click", takeBack);
  $("resign").addEventListener("click", onResign);
  $("copy-pgn").addEventListener("click", copyPgn);
  $("flip").addEventListener("click", () => {
    game.orientation = opposite(game.orientation);
    hidePromotion();
    buildBoard();
    render();
    save();
  });

  // The dialogs act on the button pressed, as the form is submitted; the
  // form then closes its dialog by itself (method="dialog").
  $("new-game-form").addEventListener("submit", (event) => {
    if (event.submitter?.value !== "start") return;
    const form = event.currentTarget;
    newGame(form.elements.color.value, form.elements.level.value);
  });
  $("result-dialog").querySelector("form").addEventListener("submit", (event) => {
    const value = event.submitter?.value;
    if (value === "pgn") copyPgn();
    // Once this dialog has closed.
    if (value === "new") setTimeout(openNewGame);
  });
}

async function start() {
  buildLevels();
  load();
  buildBoard();
  bind();
  render();
  try {
    await refresh();
  } catch (error) {
    if (error.message.startsWith("Motor")) throw error;
    // A saved game the engine does not accept: start a new one.
    game.moves = [];
    game.resigned = false;
    await refresh();
  }
  game.ready = true;
  render();
  if (!isOver() && !playersTurn()) await engineMove();
}

run(start);

// Offline use and installing as an app. Not while developing on this
// machine, where stored copies would hide every change.
const local = ["localhost", "127.0.0.1", "[::1]"].includes(location.hostname);
if ("serviceWorker" in navigator && !local) {
  navigator.serviceWorker.register("sw.js").catch((error) => console.warn("Service worker:", error));
}
