// neko-overlays-plugin: the info bar player. Runs inside the OBS browser source that the plugin creates, and in the style gallery.
// A port of neko-overlays' src/scripts/infobar.ts: same rotation, same fade, same train, same "only a higher event number is new" rule.
// Differences: it talks to the plugin on this computer (/api/bar/<id>/state, polled every second), the settings arrive with the
// state (so a change in the OBS properties applies without reloading), and the icons are inline SVGs instead of Font Awesome.
// Everything that reaches the page is built with elements and textContent, never innerHTML.
(() => {
  'use strict';

  const FADE_MS = 400; // matches the transition in _bar.scss
  const TRAIN_TICK_MS = 1000;
  const POLL_MS = 1000;

  const COUNT_KEYS = ['followers', 'subs', 'viewers'];
  const LATEST_KEYS = ['follower', 'subscriber', 'cheer', 'streaker'];

  const params = new URLSearchParams(location.search);
  const barId = location.pathname.split('/').pop() || '0';
  const demoMode = params.get('demo') === '1';
  const themeOverride = params.get('theme');

  const root = document.getElementById('info-bar');

  const DEFAULT_SETTINGS = {
    theme: 'classic', colors: {}, font: null, fontSize: null, googleFonts: true, rotateSeconds: 10,
    train: { enabled: true, type: 'subscriber', seconds: 30 },
    stats: { followers: true, subs: true, viewers: false },
    latest: { follower: true, subscriber: true, cheer: true, streaker: true },
  };

  // ---------- icons (simple stand-ins for the Font Awesome ones) ----------
  const SVG_NS = 'http://www.w3.org/2000/svg';
  const ICON_PATHS = {
    heart: 'M12 21.35l-1.45-1.32C5.4 15.36 2 12.28 2 8.5 2 5.42 4.42 3 7.5 3c1.74 0 3.41.81 4.5 2.09C13.09 3.81 14.76 3 16.5 3 19.58 3 22 5.42 22 8.5c0 3.78-3.4 6.86-8.55 11.54L12 21.35z',
    crown: 'M3 8l4.5 4L12 4l4.5 8L21 8l-2 10H5L3 8zm2 12h14v2H5v-2z',
    eye: 'M12 4.5C7 4.5 2.73 7.61 1 12c1.73 4.39 6 7.5 11 7.5s9.27-3.11 11-7.5c-1.73-4.39-6-7.5-11-7.5zM12 17c-2.76 0-5-2.24-5-5s2.24-5 5-5 5 2.24 5 5-2.24 5-5 5zm0-8c-1.66 0-3 1.34-3 3s1.34 3 3 3 3-1.34 3-3-1.34-3-3-3z',
    star: 'M12 17.27L18.18 21l-1.64-7.03L22 9.24l-7.19-.61L12 2 9.19 8.63 2 9.24l5.46 4.73L5.82 21z',
    fire: 'M12 2c.6 3.4-2.2 5-3.6 7.6C7 12 7.6 14.6 9 16c-.1-1.6.6-2.6 1.6-3.6C12 11 13 9.4 12 2zm2.6 8.2c.3 2.4-1.6 3.6-1.6 6 0 1.4.8 2.4 1.8 2.8C17 18.4 18 16 17.4 13.6c-.4-1.6-1.4-2.4-2.8-3.4zM12 22a6.5 6.5 0 0 1-6.5-6.5c0-1.6.5-3 1.4-4.3.1 2.6 1.6 4.4 3.2 5.3C9.5 14 11 11.6 12.4 10.4c0 2.6 1.9 3.5 1.9 6.1 0 2-1.1 3.5-2.3 5.5z',
  };
  const ICONS = { followers: 'heart', subs: 'crown', viewers: 'eye', follower: 'heart', subscriber: 'crown', cheer: 'star', streaker: 'fire' };
  const EMPTY = { follower: 'No Followers', subscriber: 'No Subscribers', cheer: 'No Cheers', streaker: 'No Streakers' };

  const el = (tag, className, text) => {
    const e = document.createElement(tag);
    e.className = className;
    if (text !== undefined) e.textContent = text;
    return e;
  };
  const icon = (name) => {
    const svg = document.createElementNS(SVG_NS, 'svg');
    svg.setAttribute('class', 'fas');
    svg.setAttribute('viewBox', '0 0 24 24');
    svg.setAttribute('aria-hidden', 'true');
    const path = document.createElementNS(SVG_NS, 'path');
    path.setAttribute('d', ICON_PATHS[name]);
    svg.append(path);
    return svg;
  };
  const accent = (text) => el('span', 'accent', text);
  const line = (...parts) => {
    const e = el('span', 'text');
    parts.forEach((p, i) => { if (i > 0) e.append(' '); e.append(p); });
    return e;
  };
  const counted = (n, one, many) => `${n} ${n > 1 ? many : one}`;

  // ---------- state ----------
  let settings = structuredClone(DEFAULT_SETTINGS);
  let version = null;
  let state = null; // { counts, latest }, null until the first answer
  let shownStatus = '';
  let seenSeq = null; // null until the first answer: what was already there when the bar loaded is not new

  let timers = [];
  let statsEl = null, latestEl = null, trainCountEl = null, trainBarEl = null;
  let statKeys = [], latestKeys = [];
  let statPos = 0, latestPos = 0;
  let shownStat = null, shownLatest = null;
  let trainWidth = 0; // percent
  let trainCount = 0;

  const withOverride = (s) => (themeOverride ? { ...s, theme: themeOverride } : s);

  function cssVars(s) {
    const parts = [];
    if (s.colors.accent) parts.push(`--accent-color:${s.colors.accent}`);
    if (s.colors.background) parts.push(`--background-color:${s.colors.background}`);
    if (s.colors.text) parts.push(`--text-color:${s.colors.text}`);
    if (s.colors.textAccent) parts.push(`--text-accent-color:${s.colors.textAccent}`);
    if (s.font) parts.push(`--font:'${s.font}', sans-serif`);
    if (s.fontSize) parts.push(`--font-size:${s.fontSize}px`);
    return parts.join(';');
  }

  // ---------- the text of one stat / event ----------
  const statLine = (key) => line(String(state?.counts[key] ?? 0), icon(ICONS[key]));
  function latestLine(key) {
    const event = state?.latest[key] ?? null;
    if (!event) return line(EMPTY[key], icon(ICONS[key]));
    switch (key) {
      case 'subscriber': return line(event.name, accent(counted(event.amount ?? 1, 'Month', 'Months')), icon(ICONS[key]));
      case 'cheer': return line(event.name, accent(counted(event.amount ?? 1, 'Bit', 'Bits')), icon(ICONS[key]));
      case 'streaker': return line(event.name, accent(counted(event.amount ?? 1, 'Stream', 'Streams')), icon(ICONS[key]));
      default: return line(event.name, icon(ICONS[key]));
    }
  }

  // ---------- the bar ----------
  function clearTimers() {
    timers.forEach((t) => { clearTimeout(t); clearInterval(t); });
    timers = [];
  }

  /** The slot only ever gets wider, never narrower, so the dividers don't jump around as the text changes. */
  function place(slot, content) {
    slot.replaceChildren(content);
    slot.style.width = '';
    const width = slot.getBoundingClientRect().width;
    const widest = Number(slot.dataset.widest ?? 0);
    if (width > widest) slot.dataset.widest = String(width);
    else slot.style.width = `${widest}px`;
  }
  function swap(slot, content) {
    slot.classList.add('hidden');
    timers.push(window.setTimeout(() => { place(slot, content); slot.classList.remove('hidden'); }, FADE_MS));
  }

  function rotate() {
    if (!state) return;
    if (statsEl && statKeys.length) { shownStat = statKeys[statPos]; swap(statsEl, statLine(shownStat)); statPos = (statPos + 1) % statKeys.length; }
    if (latestEl && latestKeys.length) { shownLatest = latestKeys[latestPos]; swap(latestEl, latestLine(shownLatest)); latestPos = (latestPos + 1) % latestKeys.length; }
  }

  function drawTrain() {
    if (trainCountEl) trainCountEl.textContent = String(trainCount);
    if (trainBarEl) trainBarEl.style.width = `${trainWidth}%`;
  }
  function bumpTrain() {
    if (!settings.train.enabled) return;
    trainWidth = 100;
    trainCount++;
    drawTrain();
  }

  function dress() {
    root.dataset.theme = settings.theme;
    root.setAttribute('style', cssVars(settings));
    ensureFonts();
  }

  // ---------- Google fonts ----------
  // The page never talks to Google. It asks the plugin (/api/font), which downloads the font once into its own folder;
  // when that is done the page reloads /fonts.css and the font is served from this computer.
  const SYSTEM_FONTS = new Set([
    'system-ui', 'ui-monospace', 'ui-sans-serif', 'ui-serif', '-apple-system', 'blinkmacsystemfont', 'sans-serif', 'serif',
    'monospace', 'cursive', 'fantasy', 'segoe ui', 'roboto', 'tahoma', 'geneva', 'georgia', 'menlo', 'consolas',
    'courier new', 'arial narrow', 'arial', 'helvetica', 'verdana', 'ms sans serif', 'times new roman',
  ]);
  const fontsAsked = new Set();

  /** The font names the current style uses, as the page really sees them (the style's own, or the one typed in OBS). */
  function fontNames() {
    const stack = getComputedStyle(root).getPropertyValue('--font');
    return stack.split(',')
      .map((name) => name.trim().replace(/^['"]|['"]$/g, ''))
      .filter((name) => name && /^[A-Za-z0-9 ]{1,40}$/.test(name) && !SYSTEM_FONTS.has(name.toLowerCase()));
  }

  /** Loads /fonts.css again. The new link is in place before the old one goes, so the text does not flash. */
  function reloadFontCss() {
    return new Promise((resolve) => {
      const old = document.querySelector('link[data-fonts]');
      const next = document.createElement('link');
      next.rel = 'stylesheet';
      next.dataset.fonts = '1';
      next.href = `/fonts.css?v=${Date.now()}`;
      next.onload = () => { old?.remove(); resolve(); };
      next.onerror = () => { next.remove(); resolve(); };
      document.head.append(next);
    });
  }

  async function fetchFont(name) {
    for (let tries = 0; tries < 90; tries++) {
      try {
        const res = await fetch(`/api/font?family=${encodeURIComponent(name)}`, { cache: 'no-store', signal: AbortSignal.timeout(5000) });
        const answer = await res.json();
        if (answer.status === 'ready') {
          await reloadFontCss();
          try { await Promise.race([document.fonts.load(`1em "${name}"`), new Promise((r) => setTimeout(r, 3000))]); } catch { /* shown in the fallback until it is there */ }
          // The slots were measured with the fallback font: build the bar again so they are measured with this one.
          if (state && fontNames().includes(name)) mount();
          return;
        }
        if (answer.status === 'failed') return; // not a Google font, or no connection: the style's fallback fonts are used
      } catch { /* the plugin is busy for a moment: try again */ }
      await new Promise((r) => setTimeout(r, 1000));
    }
  }

  function ensureFonts() {
    if (!settings.googleFonts) return;
    for (const name of fontNames()) {
      if (fontsAsked.has(name)) continue;
      fontsAsked.add(name);
      void fetchFont(name);
    }
  }

  /** (Re)builds the bar for the current settings. Slots that are switched off are left out, with their divider. */
  function mount() {
    clearTimers();
    dress();

    statKeys = COUNT_KEYS.filter((k) => settings.stats[k]);
    latestKeys = LATEST_KEYS.filter((k) => settings.latest[k]);
    statPos = latestPos = 0;
    shownStat = shownLatest = null;

    statsEl = statKeys.length ? el('div', 'stats-container component fade') : null;
    latestEl = latestKeys.length ? el('div', 'latest-container component fade') : null;
    const trainBox = settings.train.enabled ? el('div', 'train-container component') : null;
    trainCountEl = trainBox ? el('span', 'text train', String(trainCount)) : null;
    if (trainBox && trainCountEl) trainBox.append(trainCountEl);
    trainBarEl = settings.train.enabled ? el('div', 'train-bar') : null;

    const main = el('div', 'main-container');
    const slots = [statsEl, latestEl, trainBox].filter((s) => s !== null);
    slots.forEach((slot, i) => { if (i > 0) main.append(el('div', 'spacer')); main.append(slot); });
    root.replaceChildren(main, ...(trainBarEl ? [trainBarEl] : []));

    if (settings.train.enabled) {
      timers.push(window.setInterval(() => {
        trainWidth -= (TRAIN_TICK_MS / (settings.train.seconds * 1000)) * 100;
        if (trainWidth <= 0) { trainWidth = 0; trainCount = 0; }
        drawTrain();
      }, TRAIN_TICK_MS));
    }
    drawTrain();

    if (state) {
      rotate(); // show something real straight away
      timers.push(window.setInterval(rotate, settings.rotateSeconds * 1000));
    }
  }

  /** While the plugin has something to say (log in, an error) the bar shows that and nothing else. */
  function showStatus(status) {
    clearTimers();
    dress();
    statsEl = latestEl = trainCountEl = trainBarEl = null;
    const slot = el('div', 'latest-container component');
    slot.append(line(status.label, accent(status.value)));
    const main = el('div', 'main-container');
    main.append(slot);
    root.replaceChildren(main);
  }

  // ---------- new data ----------
  // Only a HIGHER event number counts as a new event, so a slightly old answer can never run the train twice.
  function receive(next) {
    const trainKey = settings.train.type === 'follower' ? 'follower' : 'subscriber';
    const seq = next.latest[trainKey]?.seq ?? 0;
    if (seenSeq !== null && seq > seenSeq) bumpTrain();
    seenSeq = Math.max(seenSeq ?? 0, seq);

    const previous = state;
    state = next;
    if (!previous) {
      mount(); // first answer: build the bar and start rotating
      return;
    }
    // LIVE: the number on screen changes the moment the data does, without waiting for the next rotation
    if (statsEl && shownStat && previous.counts[shownStat] !== next.counts[shownStat]) place(statsEl, statLine(shownStat));
    // A new event is shown straight away (then the rotation carries on from the slot after it)
    const fresh = latestKeys.filter((k) => (next.latest[k]?.seq ?? 0) > (previous.latest[k]?.seq ?? 0));
    if (latestEl && fresh.length) {
      shownLatest = fresh[fresh.length - 1];
      swap(latestEl, latestLine(shownLatest));
      latestPos = (latestKeys.indexOf(shownLatest) + 1) % latestKeys.length;
    } else if (latestEl && shownLatest && JSON.stringify(previous.latest[shownLatest]) !== JSON.stringify(next.latest[shownLatest])) {
      place(latestEl, latestLine(shownLatest));
    }
  }

  function handle(data) {
    const changed = data.v !== version;
    if (changed) { version = data.v; settings = withOverride(data.settings); }
    const next = { counts: data.counts, latest: data.latest };

    if (data.status) {
      const key = `${data.status.label}|${data.status.value}`;
      if (key !== shownStatus || changed) { shownStatus = key; showStatus(data.status); }
      state = null; // so the bar is built from scratch when the status goes away
      seenSeq = null;
      return;
    }
    if (shownStatus) { shownStatus = ''; state = null; seenSeq = null; }

    if (changed && state) { // the settings were changed in OBS: rebuild with them, keep what is already known
      state = next;
      mount();
      return;
    }
    receive(next);
  }

  // ---------- transport ----------
  async function poll() {
    try {
      const res = await fetch(`/api/bar/${encodeURIComponent(barId)}/state`, { cache: 'no-store', signal: AbortSignal.timeout(5000) });
      if (res.ok) handle(await res.json());
    } catch { /* the plugin is busy for a moment: try again */ }
    setTimeout(poll, POLL_MS);
  }

  // ---------- demo (the style gallery, or ?demo=1 in any browser) ----------
  const demoState = () => ({
    counts: { followers: 1284, subs: 86, viewers: 42 },
    latest: {
      follower: { name: 'NekoFan', seq: 1 },
      subscriber: { name: 'Relfy10', amount: 12, seq: 1 },
      cheer: { name: 'SomeoneNice', amount: 500, seq: 1 },
      streaker: { name: 'SogFan', amount: 31, seq: 1 },
    },
  });

  function startDemo() {
    settings = withOverride(structuredClone(DEFAULT_SETTINGS));
    settings.rotateSeconds = 4;
    receive(demoState());
    window.setInterval(() => { // a test subscriber now and then, so the train and the live update can be seen
      const next = structuredClone(state);
      next.latest.subscriber = { name: 'TestViewer', amount: 3, seq: (state.latest.subscriber?.seq ?? 0) + 1 };
      next.counts.subs++;
      receive(next);
    }, 12000);
  }

  if (demoMode) startDemo();
  else { dress(); void poll(); }
})();