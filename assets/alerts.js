(() => {
  'use strict';

  const POLL_MS = 500;
  const ENTER_MS = 450;
  const EXIT_MS = 400;
  const GAP_MS = 500;
  const MAX_QUEUE = 30;
  const STALE_MS = 5 * 60 * 1000;
  const LOAD_WAIT_MS = 3000;
  const VIDEO_MAX_MS = 120 * 1000;

  const params = new URLSearchParams(location.search);
  const alertsId = location.pathname.split('/').pop() || '0';
  const demoMode = params.get('demo') === '1';
  const themeOverride = params.get('theme');

  const root = document.getElementById('alerts-overlay');
  const stage = document.getElementById('alert-stage');

  const DEFAULT_SETTINGS = {
    theme: 'classic', colors: {}, font: null, fontSize: null, googleFonts: true,
    durationSeconds: 8, durations: {}, media: {}, soundVolume: 70, showText: true,
    types: { follow: true, sub: true, gift: true, cheer: true, raid: true },
    minBits: 1, minRaiders: 1, showMessage: true,
  };

  // Which switch in the properties controls which kind of alert.
  const TYPE_OF = { follow: 'follow', sub: 'sub', resub: 'sub', gift: 'gift', mysterygift: 'gift', cheer: 'cheer', raid: 'raid' };

  // Small plain SVG icons (24x24, drawn with currentColor), one per kind.
  const ICONS = {
    follow: 'M12 21.35l-1.45-1.32C5.4 15.36 2 12.28 2 8.5 2 5.42 4.42 3 7.5 3c1.74 0 3.41.81 4.5 2.09C13.09 3.81 14.76 3 16.5 3 19.58 3 22 5.42 22 8.5c0 3.78-3.4 6.86-8.55 11.54L12 21.35z',
    sub: 'M12 17.27L18.18 21l-1.64-7.03L22 9.24l-7.19-.61L12 2 9.19 8.63 2 9.24l5.46 4.73L5.82 21z',
    gift: 'M3 9h18v4H3z M4 14h16v7H4z M12 9L8 5.5Q7 4 8.5 3.5Q11 3.5 12 9z M12 9l4-3.5Q17 4 15.5 3.5Q13 3.5 12 9z',
    cheer: 'M6 3h12l4 6-10 12L2 9z',
    raid: 'M5 2h2v20H5z M8 3h12l-3.5 4.5L20 12H8z',
  };
  const SVG_NS = 'http://www.w3.org/2000/svg';

  const el = (tag, className, text) => {
    const e = document.createElement(tag);
    if (className) e.className = className;
    if (text !== undefined) e.textContent = text;
    return e;
  };

  let settings = structuredClone(DEFAULT_SETTINGS);
  let version = null;
  let lastSeq = null;
  const queue = [];
  let playing = false;
  let noticeEl = null;

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

  const tierText = (tier) => {
    if (tier === 'Prime') return 'Prime';
    if (tier === '2000') return 'Tier 2';
    if (tier === '3000') return 'Tier 3';
    return 'Tier 1';
  };

  const plural = (n, one, many) => `${n.toLocaleString()} ${n === 1 ? one : many}`;

  // Cheermotes arrive as words like "Cheer100" or "Kappa25": the number is the bits, not part of what the viewer said.
  const stripCheermotes = (text) => text.split(/\s+/).filter((word) => !/^[A-Za-z]+\d+$/.test(word)).join(' ').trim();

  // What an alert says: a small label on top, the name in large letters, a detail line and optionally the viewer's message.
  function describe(event) {
    const amount = Number.isFinite(event.amount) ? event.amount : 0;
    switch (event.kind) {
      case 'follow':
        return { label: 'New follower', detail: '' };
      case 'sub':
        return { label: 'New subscriber', detail: tierText(event.tier) };
      case 'resub':
        return { label: 'Resubscribed', detail: `${tierText(event.tier)}, ${plural(amount, 'month', 'months')}` };
      case 'gift':
        return { label: 'Gifted a sub', detail: event.extra ? `to ${event.extra}` : tierText(event.tier) };
      case 'mysterygift':
        return { label: 'Gifted subs', detail: `${plural(amount, 'sub', 'subs')}, ${tierText(event.tier)}` };
      case 'cheer':
        return { label: 'Cheered', detail: plural(amount, 'bit', 'bits') };
      case 'raid':
        return { label: 'Raid', detail: `with ${plural(amount, 'viewer', 'viewers')}` };
      default:
        return null;
    }
  }

  function wanted(event) {
    const type = TYPE_OF[event.kind];
    if (!type || settings.types[type] === false) return false;
    if (event.kind === 'cheer' && (event.amount ?? 0) < settings.minBits) return false;
    if (event.kind === 'raid' && (event.amount ?? 0) < settings.minRaiders) return false;
    return true;
  }

  function icon(kind) {
    const svg = document.createElementNS(SVG_NS, 'svg');
    svg.setAttribute('viewBox', '0 0 24 24');
    svg.setAttribute('class', 'icon');
    svg.setAttribute('aria-hidden', 'true');
    const path = document.createElementNS(SVG_NS, 'path');
    path.setAttribute('d', ICONS[TYPE_OF[kind]] ?? ICONS.follow);
    path.setAttribute('fill', 'currentColor');
    svg.append(path);
    return svg;
  }

  // Custom files picked in the properties, per alert type ("sub" also covers resubs, "gift" also mass gifts).
  const mediaFor = (event) => settings.media?.[TYPE_OF[event.kind]] ?? {};
  const mediaUrl = (event, slot) =>
    `/api/alerts/${encodeURIComponent(alertsId)}/media/${TYPE_OF[event.kind]}/${slot}?v=${encodeURIComponent(version)}`;

  function buildVisual(event) {
    const kind = mediaFor(event).visual;
    if (kind === 'image') {
      const img = el('img', 'media');
      img.alt = '';
      img.onerror = () => img.remove();
      img.src = mediaUrl(event, 'visual');
      return { node: img };
    }
    if (kind === 'video') {
      const video = el('video', 'media');
      video.playsInline = true;
      video.preload = 'auto';
      video.muted = false;
      video.volume = Math.min(1, Math.max(0, settings.soundVolume / 100));
      video.onerror = () => video.remove();
      video.src = mediaUrl(event, 'visual');
      return { node: video, video };
    }
    return null;
  }

  // Resolves when the picture is decoded or the video can start, or after a few seconds if it never loads.
  function whenReady(visual) {
    const loaded = visual.video
      ? new Promise((resolve) => {
        visual.video.addEventListener('loadeddata', resolve, { once: true });
        visual.video.addEventListener('error', resolve, { once: true });
      })
      : visual.node.decode().catch(() => { });
    return Promise.race([loaded, sleep(LOAD_WAIT_MS)]);
  }

  function playSound(event) {
    if (!mediaFor(event).sound || settings.soundVolume <= 0) return;
    const audio = new Audio(mediaUrl(event, 'sound'));
    audio.volume = Math.min(1, Math.max(0, settings.soundVolume / 100));
    audio.play().catch(() => { });
  }

  // A video keeps the card up until it has ended, even when that is longer than the alert time.
  function videoEnded(video) {
    return new Promise((resolve) => {
      video.addEventListener('ended', resolve, { once: true });
      video.addEventListener('error', resolve, { once: true });
      setTimeout(resolve, VIDEO_MAX_MS);
    });
  }

  // The viewer's own words, when the switch is on and the alert type carries one (resubs, subs, cheers).
  function messageText(event) {
    if (!settings.showMessage || !(event.kind === 'resub' || event.kind === 'sub' || event.kind === 'cheer')) return '';
    const message = typeof event.message === 'string' ? event.message.trim() : '';
    return event.kind === 'cheer' ? stripCheermotes(message) : message;
  }

  // THE FINALS look is not a plate with a label and a name but one line of text, like the game's "ELIMINATED <name>" banner
  // (the same as the web version's alert): the name is a span painted in the viewer's own Twitch colour, numbers and gift
  // recipients are spans in the yellow, and a short caption under the line comes from CSS (data-type and data-amount).
  // A part is a plain string, or [what, text] where what is "name" or "value".
  function finalsLine(event) {
    const amount = Number.isFinite(event.amount) ? event.amount : 0;
    const who = ['name', event.name || 'Someone'];
    const count = ['value', amount.toLocaleString()];
    switch (event.kind) {
      case 'follow': return [who, ' just followed!'];
      case 'sub': return [who, ' subscribed!'];
      case 'resub': return [who, ' subscribed for ', count, amount === 1 ? ' month!' : ' months!'];
      case 'gift': return event.extra ? [who, ' gifted a sub to ', ['value', event.extra], '!'] : [who, ' gifted a sub!'];
      case 'mysterygift': return [who, ' gifted ', count, amount === 1 ? ' sub!' : ' subs!'];
      case 'cheer': return [who, ' cheered ', count, amount === 1 ? ' bit!' : ' bits!'];
      case 'raid': return [who, ' is raiding with ', count, amount === 1 ? ' viewer!' : ' viewers!'];
      default: return [who];
    }
  }

  function buildFinalsCard(event, visual, mediaOnly) {
    const card = el('div', `card finals-card kind-${TYPE_OF[event.kind] ?? 'follow'}${mediaOnly ? ' media-only' : ''}`);
    card.dataset.type = event.kind === 'resub' ? 'sub' : event.kind;
    if (Number.isFinite(event.amount)) card.dataset.amount = event.amount.toLocaleString();
    // only a plain #rrggbb is accepted as the viewer's colour
    if (typeof event.color === 'string' && /^#[0-9a-f]{6}$/i.test(event.color)) card.style.setProperty('--user-color', event.color);
    if (visual) card.append(visual.node);
    if (mediaOnly) return card;

    const line = el('div', 'line');
    for (const part of finalsLine(event)) {
      if (typeof part === 'string') {
        line.append(part);
      } else {
        const span = el('span', 'hl', part[1]);
        span.dataset.v = part[0];
        line.append(span);
      }
    }
    card.append(line);
    const message = messageText(event);
    if (message) card.append(el('div', 'message', message));
    return card;
  }

  function buildCard(event, info, visual) {
    const mediaOnly = Boolean(visual) && !settings.showText;
    if (settings.theme === 'finals') return buildFinalsCard(event, visual, mediaOnly);
    const card = el('div', `card kind-${TYPE_OF[event.kind] ?? 'follow'}${mediaOnly ? ' media-only' : ''}`);
    if (visual) card.append(visual.node);
    if (mediaOnly) return card;
    const head = el('div', 'head');
    head.append(icon(event.kind), el('span', 'label', info.label));
    card.append(head, el('div', 'name', event.name || 'Someone'));
    if (info.detail) card.append(el('div', 'detail', info.detail));

    const message = messageText(event);
    if (message) card.append(el('div', 'message', message));
    return card;
  }

  // Each alert type can have its own time; 0 or nothing set means the general one.
  const secondsFor = (event) => {
    const own = Number(settings.durations?.[TYPE_OF[event.kind]]);
    return own > 0 ? own : settings.durationSeconds;
  };

  const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

  async function playNext() {
    if (playing) return;
    playing = true;
    while (queue.length) {
      const event = queue.shift();
      const info = describe(event);
      if (!info || !wanted(event)) continue;

      const visual = buildVisual(event);
      const card = buildCard(event, info, visual);
      stage.replaceChildren(card);
      if (visual) await whenReady(visual);
      // Two frames: the card must be painted once in its starting state for the entrance to animate.
      await new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve)));
      card.classList.add('in');
      playSound(event);
      const holds = [sleep(ENTER_MS + secondsFor(event) * 1000)];
      if (visual?.video) {
        visual.video.play().catch(() => visual.video.dispatchEvent(new Event('error')));
        holds.push(videoEnded(visual.video));
      }
      await Promise.all(holds);
      card.classList.remove('in');
      card.classList.add('out');
      await sleep(EXIT_MS);
      card.remove();
      await sleep(GAP_MS);
    }
    playing = false;
  }

  function enqueue(events) {
    const now = Date.now();
    for (const event of events) {
      if (!demoMode && Number.isFinite(event.ts) && now - event.ts > STALE_MS) continue;
      queue.push(event);
    }
    while (queue.length > MAX_QUEUE) queue.shift();
    void playNext();
  }

  function showNotice(status) {
    if (!status) { noticeEl?.remove(); noticeEl = null; return; }
    if (!noticeEl) { noticeEl = el('div', 'notice'); root.append(noticeEl); }
    noticeEl.textContent = `${status.label}: ${status.value}`;
  }

  function dress() {
    root.dataset.theme = settings.theme;
    root.setAttribute('style', cssVars(settings));
    ensureFonts();
  }

  function applySettings(next) {
    settings = { ...structuredClone(DEFAULT_SETTINGS), ...next };
    settings.types = { ...DEFAULT_SETTINGS.types, ...(next.types ?? {}) };
    dress();
  }

  const SYSTEM_FONTS = new Set([
    'system-ui', 'ui-monospace', 'ui-sans-serif', 'ui-serif', '-apple-system', 'blinkmacsystemfont', 'sans-serif', 'serif',
    'monospace', 'cursive', 'fantasy', 'segoe ui', 'roboto', 'tahoma', 'geneva', 'georgia', 'menlo', 'consolas',
    'courier new', 'arial narrow', 'arial', 'helvetica', 'verdana', 'ms sans serif', 'times new roman',
  ]);
  const fontsAsked = new Set();

  function fontNames() {
    const stack = getComputedStyle(root).getPropertyValue('--font');
    return stack.split(',')
      .map((name) => name.trim().replace(/^['"]|['"]$/g, ''))
      .filter((name) => name && /^[A-Za-z0-9 ]{1,40}$/.test(name) && !SYSTEM_FONTS.has(name.toLowerCase()));
  }

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
          try { await Promise.race([document.fonts.load(`1em "${name}"`), new Promise((r) => setTimeout(r, 3000))]); } catch { }
          return;
        }
        if (answer.status === 'failed') return;
      } catch { }
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

  function handle(data) {
    if (data.v !== version) { version = data.v; applySettings(withOverride(data.settings)); }
    showNotice(data.status);

    const events = data.events ?? [];
    const newest = events.length ? events[events.length - 1].seq : 0;
    if (lastSeq === null || newest < lastSeq) {
      // First answer after the page opens (or the plugin restarted): what is already in the log has been shown or missed.
      lastSeq = newest;
      return;
    }
    const fresh = events.filter((event) => event.seq > lastSeq);
    lastSeq = newest;
    if (fresh.length) enqueue(fresh);
  }

  async function poll() {
    try {
      const res = await fetch(`/api/alerts/${encodeURIComponent(alertsId)}/state`, { cache: 'no-store', signal: AbortSignal.timeout(5000) });
      if (res.ok) handle(await res.json());
    } catch { }
    setTimeout(poll, POLL_MS);
  }

  const DEMO_EVENTS = [
    { kind: 'follow', name: 'NekoFan', color: '#8a5cf5' },
    { kind: 'sub', name: 'Relfy10', amount: 1, tier: '1000', message: '' },
    { kind: 'cheer', name: 'SogFan', amount: 500, message: 'Cheer500 take my bits', color: '#1e90ff' },
    { kind: 'resub', name: 'SomeoneNice', amount: 12, tier: '2000', message: 'A whole year already', color: '#00ff7f' },
    { kind: 'gift', name: 'NekoFan', amount: 1, tier: '1000', extra: 'Relfy10', color: '#8a5cf5' },
    { kind: 'mysterygift', name: 'SogFan', amount: 5, tier: '1000', color: '#ff4500' },
    { kind: 'raid', name: 'SomeoneNice', amount: 42, color: '#00ff7f' },
  ];

  function startDemo() {
    applySettings(withOverride(structuredClone(DEFAULT_SETTINGS)));
    settings.durationSeconds = 2.5;
    let n = 0;
    const next = () => { enqueue([{ ...DEMO_EVENTS[n % DEMO_EVENTS.length], seq: n + 1, ts: Date.now() }]); n++; };
    next();
    window.setInterval(() => { if (queue.length < 2) next(); }, 3500);
  }

  if (demoMode) startDemo();
  else { dress(); void poll(); }
})();