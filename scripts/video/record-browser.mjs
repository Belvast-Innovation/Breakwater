// Record the browser segments of the demo video with Playwright (headless Chromium).
//
//   node scripts/video/record-browser.mjs [name ...]
//
// Writes breakwater-out/video/raw/<name>.webm at 1920x1080, one slowly scrolling
// page per segment. Run from the repository root with `playwright` resolvable
// (scripts/video/make-segments.sh installs it under breakwater-out/video/node).
import { chromium } from "playwright";
import { mkdirSync, readdirSync, renameSync, rmSync, writeFileSync } from "node:fs";
import { join, resolve } from "node:path";
import { pathToFileURL } from "node:url";

const ROOT = process.cwd();
const RAW = join(ROOT, "breakwater-out/video/raw");
const SIZE = { width: 1920, height: 1080 };

// Author names appear in GitHub's latest-commit bar and contributors list; they are hidden.
const GITHUB_CSS = `a[href="/hannesgao"], a[href^="/hannesgao?"], img[alt="@hannesgao"], img[alt="hannesgao"],
  [data-testid="author-avatar"], [data-testid="latest-commit"] a[href*="/commits?author"],
  .BorderGrid-row:has(a[href$="/graphs/contributors"]) { visibility: hidden !important; }`;

const TARGETS = {
  // Segment 04: scroll to the violation chart, then to the failing step (#2) in the step
  // table. See findingsTour below.
  "report-policy-bypass-findings": {
    url: pathToFileURL(resolve(ROOT, "docs/reports/policy-bypass-seed-7.html")).href,
    zoom: 2, hold: 1000, tour: "findings",
  },
  "report-policy-bypass": {
    url: pathToFileURL(resolve(ROOT, "docs/reports/policy-bypass-seed-7.html")).href,
    zoom: 1.5, hold: 2500, scrollSeconds: 12,
  },
  "report-ttl-archival": {
    url: pathToFileURL(resolve(ROOT, "docs/reports/ttl-archival-seed-11.html")).href,
    zoom: 1.5, hold: 1500, scrollSeconds: 9,
  },
  "stellar-expert": {
    url: "https://stellar.expert/explorer/testnet/tx/3f30f306985585ff62b31271f7aa050a26ec82f6458925d17aa5c32ce157aa8e",
    zoom: 1.5, hold: 10000, scrollSeconds: 9, settle: 6000,
  },
  // Segment 01: the README's Problem section on GitHub, scrolled slowly from its heading.
  "readme-problem": {
    url: "https://github.com/Belvast-Innovation/Breakwater",
    zoom: 1.25, hold: 1500, scrollSeconds: 13, settle: 3000,
    anchor: "#user-content-problem", scrollPx: 900,
    css: GITHUB_CSS,
  },
  github: {
    url: "https://github.com/Belvast-Innovation/Breakwater",
    zoom: 1.25, hold: 3000, scrollSeconds: 19, settle: 3000,
    css: GITHUB_CSS,
  },
};

const wanted = process.argv.slice(2).length ? process.argv.slice(2) : Object.keys(TARGETS);
mkdirSync(RAW, { recursive: true });
// PLAYWRIGHT_CHANNEL=chrome uses an installed Google Chrome instead of Playwright's bundled Chromium.
const browser = await chromium.launch(process.env.PLAYWRIGHT_CHANNEL ? { channel: process.env.PLAYWRIGHT_CHANNEL } : {});

for (const name of wanted) {
  const t = TARGETS[name];
  if (!t) throw new Error(`unknown target ${name}`);
  const dir = join(RAW, `${name}.tmp`);
  rmSync(dir, { recursive: true, force: true });
  const context = await browser.newContext({
    viewport: SIZE, deviceScaleFactor: 1, colorScheme: "dark",
    recordVideo: { dir, size: SIZE },
  });
  const t0 = Date.now();
  const page = await context.newPage();
  await page.goto(t.url, { waitUntil: "load", timeout: 60000 });
  if (t.settle) await page.waitForTimeout(t.settle);
  if (t.css) await page.addStyleTag({ content: t.css });
  // Instant scrolling: a page's own smooth scrolling would fight the animation below.
  await page.evaluate((z) => {
    document.documentElement.style.zoom = String(z);
    document.documentElement.style.scrollBehavior = "auto";
  }, t.zoom);
  if (t.anchor) {
    await page.evaluate((sel) => {
      document.querySelector(sel).scrollIntoView({ block: "start" });
      window.scrollBy(0, -60);
    }, t.anchor);
  }
  await page.waitForTimeout(500);
  // Everything before this point is page loading; make-segments.sh cuts it off.
  const start = (Date.now() - t0) / 1000;
  await page.waitForTimeout(t.hold);

  if (t.tour === "findings") {
    await page.evaluate(findingsTour);
  } else {
    await scrollDown(page, t.scrollSeconds, t.scrollPx);
  }
  await page.waitForTimeout(2000);
  const length = (Date.now() - t0) / 1000 - start;
  await context.close();
  writeFileSync(join(RAW, `${name}.json`), JSON.stringify({ start, length }));

  const [file] = readdirSync(dir).filter((f) => f.endsWith(".webm"));
  renameSync(join(dir, file), join(RAW, `${name}.webm`));
  rmSync(dir, { recursive: true, force: true });
  console.log(`recorded ${name}`);
}
await browser.close();

// Ease-in-out scroll, animated inside the page so it runs in real time: by `px` from the
// current position, or to the bottom of the page when `px` is not given.
async function scrollDown(page, seconds, px) {
  await page.evaluate(([ms, px]) => new Promise((done) => {
    const el = document.scrollingElement || document.documentElement;
    const from = el.scrollTop;
    const distance = px ?? Math.max(0, el.scrollHeight - window.innerHeight - from);
    const begin = performance.now();
    const step = (now) => {
      const x = Math.min(1, (now - begin) / ms);
      el.scrollTop = from + distance * (x < 0.5 ? 2 * x * x : 1 - Math.pow(-2 * x + 2, 2) / 2);
      if (x < 1) requestAnimationFrame(step); else done();
    };
    requestAnimationFrame(step);
  }), [seconds * 1000, px ?? null]);
}

// Runs inside the policy-bypass report (about 23 s): scroll to the violation chart and stay
// there, then scroll to the failing step (#2) in the step table and highlight it. Scrolling
// only; no zoom, which makes the picture shimmer.
async function findingsTour() {
  const ease = (x) => (x < 0.5 ? 2 * x * x : 1 - Math.pow(-2 * x + 2, 2) / 2);
  const animate = (ms, f) => new Promise((done) => {
    const begin = performance.now();
    const step = (now) => {
      const x = Math.min(1, (now - begin) / ms);
      f(ease(x));
      if (x < 1) requestAnimationFrame(step); else done();
    };
    requestAnimationFrame(step);
  });
  const el = document.scrollingElement || document.documentElement;
  const zoom = parseFloat(document.documentElement.style.zoom) || 1;
  const body = document.body;
  body.style.paddingBottom = "60vh"; // room to centre the chart, the last thing on the page
  // Scroll so that `target` (a page offset in CSS pixels) sits at `fraction` of the viewport.
  const scrollTo = (target, fraction, ms) => {
    const from = el.scrollTop;
    const to = Math.max(0, Math.min(el.scrollHeight - innerHeight, target * zoom - innerHeight * fraction));
    return animate(ms, (x) => { el.scrollTop = from + (to - from) * x; });
  };
  const pageY = (node) => node.getBoundingClientRect().top / zoom + el.scrollTop / zoom;

  // 1. The violation chart, in the middle of the screen.
  const chart = document.querySelector('svg[aria-label="cumulative spend"]');
  await scrollTo(pageY(chart) + chart.getBoundingClientRect().height / zoom / 2, 0.5, 2500);
  await new Promise((r) => setTimeout(r, 9300));

  // 2. The failing step in the step table, highlighted.
  const row = document.querySelector("tr.fail");
  await scrollTo(pageY(row), 0.5, 2000);
  row.style.transition = "background-color 600ms, outline-color 600ms";
  row.style.outline = "3px solid transparent";
  row.style.outlineOffset = "2px";
  await new Promise((r) => setTimeout(r, 50));
  row.style.backgroundColor = "rgba(248, 81, 73, 0.18)";
  row.style.outlineColor = "#f85149";
  await new Promise((r) => setTimeout(r, 9000));
}
