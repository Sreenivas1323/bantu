// Bantu's service worker: lets the installed app open offline-tolerantly and
// keeps its files fresh. Daemon traffic (/v1/...) is never cached, because it
// carries live state and a token.

const CACHE = "bantu-shell-v1";

self.addEventListener("install", () => self.skipWaiting());

self.addEventListener("activate", (event) => {
  event.waitUntil(
    (async () => {
      for (const key of await caches.keys()) {
        if (key !== CACHE) await caches.delete(key);
      }
      await self.clients.claim();
    })(),
  );
});

self.addEventListener("fetch", (event) => {
  const { request } = event;
  const url = new URL(request.url);
  if (request.method !== "GET" || url.origin !== self.location.origin || url.pathname.includes("/v1/")) return;

  // Built assets have content hashes in their names, so a cached copy is right.
  if (url.pathname.includes("/assets/")) {
    event.respondWith(
      (async () => {
        const cache = await caches.open(CACHE);
        const hit = await cache.match(request);
        if (hit) return hit;
        const res = await fetch(request);
        if (res.ok) await cache.put(request, res.clone());
        return res;
      })(),
    );
    return;
  }

  // The app page: fresh when online, the last copy when not.
  if (request.mode === "navigate") {
    event.respondWith(
      (async () => {
        const cache = await caches.open(CACHE);
        try {
          const res = await fetch(request);
          if (res.ok) await cache.put("app-shell", res.clone());
          return res;
        } catch (err) {
          const hit = await cache.match("app-shell");
          if (hit) return hit;
          throw err;
        }
      })(),
    );
  }
});
