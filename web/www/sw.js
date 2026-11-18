// Keeps the whole page on the device, so that it opens and plays without a
// connection, like an installed app.
//
// Every file is stored when the worker installs. The deployment replaces
// __VERSION__ with the commit it builds, so each release gets a cache of its
// own: the new worker stores the new files, takes over, and deletes the old
// cache. A game in progress is not interrupted; the next visit uses the new
// version.

const VERSION = "__VERSION__";
const CACHE = `fishrod-${VERSION}`;

const PIECES = ["K", "Q", "R", "B", "N", "P"].flatMap((type) => [`pieces/w${type}.svg`, `pieces/b${type}.svg`]);

const FILES = [
  "./",
  "index.html",
  "style.css",
  "app.js",
  "fishrod.js",
  "engine-worker.js",
  "fishrod.wasm",
  "manifest.webmanifest",
  "img/logo-96.png",
  "img/favicon-32.png",
  "img/apple-touch-icon.png",
  "img/icon-192.png",
  "img/icon-512.png",
  "img/icon-maskable-512.png",
  ...PIECES,
];

self.addEventListener("install", (event) => {
  event.waitUntil(
    caches
      .open(CACHE)
      .then((cache) => cache.addAll(FILES))
      .then(() => self.skipWaiting())
  );
});

self.addEventListener("activate", (event) => {
  event.waitUntil(
    caches
      .keys()
      .then((keys) =>
        Promise.all(keys.filter((key) => key.startsWith("fishrod-") && key !== CACHE).map((key) => caches.delete(key)))
      )
      .then(() => self.clients.claim())
  );
});

// The stored copy when there is one, the network otherwise.
self.addEventListener("fetch", (event) => {
  const request = event.request;
  if (request.method !== "GET" || new URL(request.url).origin !== self.location.origin) return;
  event.respondWith(caches.match(request, { ignoreSearch: true }).then((stored) => stored ?? fetch(request)));
});
