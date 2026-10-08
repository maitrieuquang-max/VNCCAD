// VNCCad offline service worker.
//
// Install: cache the app shell AND the hashed .wasm/.js that the current index.html references,
// so the app works offline right after the first visit.
// Fetch: index.html is network-first (updates arrive when online, cached copy when offline);
// every other same-origin file is cache-first.
// Activate: drop hashed app files that the current index.html no longer references, so old
// versions (about 13 MB each) don't pile up.
const CACHE = 'vnccad-v2';
const SHELL = ['./', './index.html', './manifest.webmanifest', './icon-192.png', './icon-512.png'];
const ASSET_RE = /cadcraft-web-[0-9a-f]+(?:_bg\.wasm|\.js)/g;

function referencedAssets(html) {
  return [...new Set(html.match(ASSET_RE) || [])].map((n) => './' + n);
}

self.addEventListener('install', (e) => {
  e.waitUntil((async () => {
    const c = await caches.open(CACHE);
    await c.addAll(SHELL);
    const res = await fetch('./index.html', { cache: 'no-cache' });
    const html = await res.clone().text();
    await c.put('./index.html', res);
    await c.addAll(referencedAssets(html));
    await self.skipWaiting();
  })());
});

// Keep only the hashed files the cached index.html references. Runs on activate and whenever a
// newer index.html arrives (a new deployment changes the hashes but not this file).
async function prune() {
  const c = await caches.open(CACHE);
  const index = await c.match('./index.html');
  if (!index) return;
  const keep = new Set(referencedAssets(await index.text()).map((p) => new URL(p, self.registration.scope).href));
  for (const req of await c.keys()) {
    if (req.url.match(ASSET_RE) && !keep.has(req.url)) await c.delete(req);
  }
}

self.addEventListener('activate', (e) => {
  e.waitUntil((async () => {
    for (const k of await caches.keys()) if (k !== CACHE) await caches.delete(k);
    await prune();
    await self.clients.claim();
  })());
});

self.addEventListener('fetch', (e) => {
  const req = e.request;
  if (req.method !== 'GET' || new URL(req.url).origin !== self.location.origin) return;
  const isShell = req.mode === 'navigate' || req.url.endsWith('/index.html');
  if (isShell) {
    e.respondWith(
      fetch(req)
        .then((res) => {
          if (res.ok) {
            const copy = res.clone();
            e.waitUntil(caches.open(CACHE).then((c) => c.put('./index.html', copy)).then(prune));
          }
          return res;
        })
        .catch(() => caches.match('./index.html'))
    );
    return;
  }
  e.respondWith(
    caches.match(req).then((hit) => hit || fetch(req).then((res) => {
      if (res.ok) { const copy = res.clone(); caches.open(CACHE).then((c) => c.put(req, copy)); }
      return res;
    }))
  );
});
