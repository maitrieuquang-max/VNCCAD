// VNCCad: run the web build's self test in a real browser (headless Chromium, WebGL through
// SwiftShader). Usage: node selftest.mjs http://localhost:8080/
// Exits 1 when the self test fails or does not finish. Writes selftest.png (a screenshot).
import { chromium } from 'playwright';

const base = process.argv[2] || 'http://localhost:8080/';
const url = new URL('?webgl&selftest&cjk', base).href;
const browser = await chromium.launch({ args: ['--use-gl=angle', '--use-angle=swiftshader', '--enable-unsafe-swiftshader', '--ignore-gpu-blocklist'] });
const context = await browser.newContext({ viewport: { width: 1400, height: 900 } });
try {
  await context.grantPermissions(['local-fonts']);
} catch (e) {
  console.log('local-fonts permission not supported here:', e.message);
}
const page = await context.newPage();
page.on('console', (m) => console.log(`[console.${m.type()}] ${m.text()}`));
page.on('pageerror', (e) => console.log(`[pageerror] ${e.message}`));
// Chrome offers to kill a page whose main thread is busy for much longer than this.
const LIMIT_MS = 20000;
let maxBlock = 0;
const t0 = Date.now();
await page.goto(url, { waitUntil: 'load' });
// The page must stay responsive while the files open: probe the main thread every second.
const probe = setInterval(async () => {
  const t = Date.now();
  try {
    await page.evaluate(() => 1, { timeout: 60000 });
    const dt = Date.now() - t;
    if (dt > maxBlock) maxBlock = dt;
    if (dt > 3000) console.log(`main thread busy for ${dt} ms`);
  } catch (e) {
    /* page closed */
  }
}, 1000);
let result = 'timeout';
try {
  const el = await page.waitForSelector('#vnccad-selftest', { state: 'attached', timeout: 420000 });
  result = await el.getAttribute('data-result');
  console.log(await el.textContent());
} catch (e) {
  console.log('self test did not finish:', e.message);
}
clearInterval(probe);
console.log(`elapsed ${Date.now() - t0} ms, longest busy main thread ${maxBlock} ms`);
const hung = maxBlock > LIMIT_MS;
await page.screenshot({ path: 'selftest.png' });
await browser.close();
if (result !== 'ok' || hung) {
  console.log(`FAILED (result=${result}, hung=${hung})`);
  process.exit(1);
}
console.log('PASSED');
