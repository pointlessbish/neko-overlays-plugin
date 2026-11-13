(() => {
  'use strict';

  const POLL_MS = 500;
  const EXIT_MS = 300;

  const params = new URLSearchParams(location.search);
  const chatId = location.pathname.split('/').pop() || '0';
  const demoMode = params.get('demo') === '1';
  const themeOverride = params.get('theme');

  const root = document.getElementById('chat-overlay');
  const list = document.getElementById('chat-messages');

  const DEFAULT_SETTINGS = {
    theme: 'classic', colors: {}, font: null, fontSize: null, googleFonts: true,
    maxMessages: 12, lifetimeSeconds: 0, twitchColors: true, badges: true, avatars: true, thirdParty: true,
    blocklist: ['streamelements', 'streamlabs'],
  };

  const LIGHT_THEMES = new Set(['win98']);
  const ROLES = [['lead_moderator', 'lead mod'], ['moderator', 'mod'], ['vip', 'vip'], ['artist-badge', 'artist']];

  const el = (tag, className, text) => {
    const e = document.createElement(tag);
    if (className) e.className = className;
    if (text !== undefined) e.textContent = text;
    return e;
  };

  let settings = structuredClone(DEFAULT_SETTINGS);
  let version = null;
  let latest = [];
  let room = '';
  let blocked = new Set();
  const shown = new Map();
  const gone = new Set();
  const timers = new Set();
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

  const thirdParty = new Map();
  const badgePictures = new Map();
  const plainId = /^[A-Za-z0-9_]+$/;

  const twitchEmoteUrl = (id) =>
    `https://static-cdn.jtvnw.net/emoticons/v2/${id}/default/${LIGHT_THEMES.has(settings.theme) ? 'light' : 'dark'}/2.0`;

  async function getJson(url) {
    const res = await fetch(url, { signal: AbortSignal.timeout(10000) });
    if (!res.ok) throw new Error(String(res.status));
    return res.json();
  }

  const https = (url) => (typeof url === 'string' && url.startsWith('//') ? `https:${url}` : typeof url === 'string' && url.startsWith('https://') ? url : null);
  const pairs = (list2) => list2.filter(([name, url]) => typeof name === 'string' && name && url);

  const parseBttv = (data) => pairs((Array.isArray(data) ? data : [...(data.channelEmotes ?? []), ...(data.sharedEmotes ?? [])])
    .filter((e) => plainId.test(String(e.id))).map((e) => [e.code, `https://cdn.betterttv.net/emote/${e.id}/2x.webp`]));
  const parseFfz = (data) => pairs(Object.values(data.sets ?? {}).flatMap((set) => (set.emoticons ?? [])
    .map((e) => [e.name, https(e.urls?.['2'] ?? e.urls?.['1'])])));
  const parse7tv = (data) => pairs((data.emotes ?? data.emote_set?.emotes ?? [])
    .filter((e) => plainId.test(String(e.id))).map((e) => [e.name, `https://cdn.7tv.app/emote/${e.id}/2x.webp`]));

  const PROVIDERS = [
    { global: 'https://api.betterttv.net/3/cached/emotes/global', channel: (id) => `https://api.betterttv.net/3/cached/users/twitch/${id}`, parse: parseBttv },
    { global: 'https://api.frankerfacez.com/v1/set/global', channel: (id) => `https://api.frankerfacez.com/v1/room/id/${id}`, parse: parseFfz },
    { global: 'https://7tv.io/v3/emote-sets/global', channel: (id) => `https://7tv.io/v3/users/twitch/${id}`, parse: parse7tv },
  ];

  let globalEmotesAsked = false;
  let channelEmotesFor = '';
  async function loadEmotes(urlOf) {
    await Promise.all(PROVIDERS.map(async (provider) => {
      try { for (const [word, url] of provider.parse(await getJson(urlOf(provider)))) thirdParty.set(word, url); } catch { }
    }));
  }

  const badgeKey = /^[A-Za-z0-9_-]+\/[A-Za-z0-9_-]+$/;
  const pendingBadges = new Map();
  let badgesFor = null;
  let badgesBusy = false;
  let badgesRetryAt = 0;

  function prunePending() {
    for (const img of pendingBadges.keys()) if (!img.isConnected) pendingBadges.delete(img);
    for (const [login, imgs] of pendingAvatars) {
      for (const img of imgs) if (!img.isConnected) imgs.delete(img);
      if (!imgs.size) pendingAvatars.delete(login);
    }
  }

  function settleBadges(ready) {
    for (const [img, key] of pendingBadges) {
      const picture = badgePictures.get(key);
      if (picture) { img.src = picture; pendingBadges.delete(img); }
      else if (ready) { img.remove(); pendingBadges.delete(img); }
    }
    prunePending();
  }

  async function loadBadges() {
    badgesBusy = true;
    const asked = room;
    let ready = false;
    try {
      const data = await getJson(`/api/badges?room=${encodeURIComponent(asked)}`);
      for (const [key, url] of Object.entries(data.badges ?? {})) {
        const picture = https(url);
        if (picture && badgeKey.test(key)) badgePictures.set(key, picture);
      }
      ready = data.ready === true;
    } catch { }
    badgesBusy = false;
    if (ready) badgesFor = asked; else badgesRetryAt = Date.now() + 10000;
    settleBadges(ready);
  }

  const avatarPictures = new Map();
  const pendingAvatars = new Map();
  const avatarAsked = new Map();
  let avatarsBusy = false;

  async function loadAvatars() {
    if (avatarsBusy) return;
    const now = Date.now();
    const logins = [...pendingAvatars.keys()].filter((login) => now - (avatarAsked.get(login) ?? 0) > 30000).slice(0, 40);
    if (!logins.length) return;
    avatarsBusy = true;
    logins.forEach((login) => avatarAsked.set(login, now));
    try {
      const data = await getJson(`/api/avatars?logins=${encodeURIComponent(logins.join(','))}`);
      for (const [login, url] of Object.entries(data)) {
        const picture = https(url);
        if (picture) avatarPictures.set(login, picture);
      }
    } catch { }
    avatarsBusy = false;
    for (const login of logins) {
      const picture = avatarPictures.get(login);
      const imgs = pendingAvatars.get(login);
      if (picture && imgs) { imgs.forEach((img) => { img.src = picture; }); pendingAvatars.delete(login); }
    }
    prunePending();
  }

  function ensureExtras() {
    if (settings.thirdParty) {
      if (!globalEmotesAsked) { globalEmotesAsked = true; void loadEmotes((p) => p.global); }
      if (/^\d+$/.test(room) && channelEmotesFor !== room) { channelEmotesFor = room; void loadEmotes((p) => p.channel(room)); }
    }
    if (settings.badges && badgesFor !== room && !badgesBusy && Date.now() >= badgesRetryAt) void loadBadges();
    if (settings.avatars && pendingAvatars.size) void loadAvatars();
  }

  function emoteImg(url, name) {
    const img = el('img', 'emote');
    img.alt = name;
    img.title = name;
    img.src = url;
    img.onerror = () => img.replaceWith(document.createTextNode(name));
    return img;
  }

  function appendWords(parent, text) {
    for (const token of text.split(/(\s+)/)) {
      if (!token) continue;
      const url = settings.thirdParty ? thirdParty.get(token) : undefined;
      parent.append(url ? emoteImg(url, token) : document.createTextNode(token));
    }
  }

  function renderBody(message) {
    const body = el('span', 'body');
    const chars = Array.from(message.text);
    let pos = 0;
    for (const e of message.emotes ?? []) {
      if (!plainId.test(String(e.id)) || e.start < pos || e.end < e.start || e.end >= chars.length) continue;
      appendWords(body, chars.slice(pos, e.start).join(''));
      body.append(emoteImg(twitchEmoteUrl(e.id), chars.slice(e.start, e.end + 1).join('')));
      pos = e.end + 1;
    }
    appendWords(body, chars.slice(pos).join(''));
    return body;
  }

  const ROLE_BADGES = ['broadcaster', 'staff', 'admin', 'global_mod', 'lead_moderator', 'moderator', 'vip'];
  function pickBadges(badges) {
    const role = ROLE_BADGES.map((set) => badges.find((badge) => badge.set === set)).find(Boolean);
    const other = badges.find((badge) => !ROLE_BADGES.includes(badge.set));
    return [role, other].filter(Boolean);
  }

  function avatarImg(login) {
    const img = el('img', 'avatar');
    img.alt = '';
    img.hidden = true;
    img.onload = () => { img.hidden = false; };
    img.onerror = () => img.remove();
    const picture = avatarPictures.get(login);
    if (picture) img.src = picture;
    else {
      if (!pendingAvatars.has(login)) pendingAvatars.set(login, new Set());
      pendingAvatars.get(login).add(img);
    }
    return img;
  }

  function buildMessage(message) {
    const node = el('div', message.action ? 'message action' : 'message');
    const color = settings.twitchColors && /^#[0-9a-fA-F]{6}$/.test(message.color) ? message.color : '';

    if (settings.avatars && /^[a-z0-9_]+$/.test(message.login ?? '')) node.append(avatarImg(message.login));

    const badges = message.badges ?? [];
    if (settings.badges) {
      for (const badge of pickBadges(badges)) {
        const key = `${badge.set}/${badge.v}`;
        const picture = badgePictures.get(key);
        if (!picture && badgesFor === room) continue;
        const img = el('img', 'badge');
        img.alt = badge.set;
        img.dataset.set = badge.set;
        img.hidden = true;
        img.onload = () => { img.hidden = false; };
        img.onerror = () => { pendingBadges.delete(img); img.remove(); };
        if (picture) img.src = picture; else pendingBadges.set(img, key);
        node.append(img);
      }
    }
    for (const [set, label] of ROLES) {
      if (badges.some((badge) => badge.set === set)) node.append(el('span', 'role', label));
    }

    const name = el('span', 'name', message.name);
    if (color) name.style.color = color;
    const body = renderBody(message);
    if (color && message.action) body.style.color = color;
    node.append(name, el('span', 'sep', ':'), ' ', body);
    return node;
  }

  function leave(seq) {
    const node = shown.get(seq);
    if (!node) return;
    shown.delete(seq);
    gone.add(seq);
    node.classList.add('leaving');
    window.setTimeout(() => node.remove(), EXIT_MS);
  }

  function add(message, instant, age) {
    const node = buildMessage(message);
    if (instant) node.classList.add('instant');
    list.append(node);
    shown.set(message.seq, node);
    if (settings.lifetimeSeconds > 0) {
      const timer = window.setTimeout(() => { timers.delete(timer); leave(message.seq); }, Math.max(0, settings.lifetimeSeconds * 1000 - age));
      timers.add(timer);
    }
  }

  function sync(instant) {
    const current = new Map(latest.map((m) => [m.seq, m]));

    for (const seq of [...shown.keys()]) {
      const message = current.get(seq);
      if (!message || blocked.has(message.login)) leave(seq);
    }
    for (const seq of [...gone]) if (!current.has(seq)) gone.delete(seq);

    const fresh = [];
    for (const message of latest) {
      if (shown.has(message.seq) || gone.has(message.seq)) continue;
      if (blocked.has(message.login)) continue;
      const age = Math.max(0, Date.now() - message.ts);
      if (settings.lifetimeSeconds > 0 && age >= settings.lifetimeSeconds * 1000) { gone.add(message.seq); continue; }
      fresh.push([message, age]);
    }
    const skip = Math.max(0, fresh.length - settings.maxMessages);
    fresh.slice(0, skip).forEach(([message]) => gone.add(message.seq));
    fresh.slice(skip).forEach(([message, age]) => add(message, instant, age));

    while (shown.size > settings.maxMessages) leave(shown.keys().next().value);
  }

  function showNotice(status) {
    if (!status) { noticeEl?.remove(); noticeEl = null; return; }
    if (!noticeEl) { noticeEl = el('div', 'message notice'); root.append(noticeEl); }
    noticeEl.textContent = `${status.label}: ${status.value}`;
  }

  function dress() {
    root.dataset.theme = settings.theme;
    root.setAttribute('style', cssVars(settings));
    ensureFonts();
  }

  function applySettings(next) {
    settings = { ...structuredClone(DEFAULT_SETTINGS), ...next };
    blocked = new Set(settings.blocklist ?? []);
    timers.forEach((t) => clearTimeout(t));
    timers.clear();
    shown.clear();
    gone.clear();
    list.replaceChildren();
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

  let firstAnswer = true;
  function handle(data) {
    const changed = data.v !== version;
    if (changed) { version = data.v; applySettings(withOverride(data.settings)); }
    room = data.room || '';
    latest = data.messages ?? [];
    showNotice(data.status);
    ensureExtras();
    sync(firstAnswer || changed);
    firstAnswer = false;
  }

  async function poll() {
    try {
      const res = await fetch(`/api/chat/${encodeURIComponent(chatId)}/state`, { cache: 'no-store', signal: AbortSignal.timeout(5000) });
      if (res.ok) handle(await res.json());
    } catch { }
    setTimeout(poll, POLL_MS);
  }

  const DEMO_EMOTES = { Kappa: '25', LUL: '425618', Kreygasm: '41', '<3': '9' };
  const DEMO_USERS = [
    { login: 'relfy10', name: 'Relfy10', color: '#ff69b4', badges: [{ set: 'subscriber', v: '12' }] },
    { login: 'nekofan', name: 'NekoFan', color: '#5fb8ff', badges: [{ set: 'moderator', v: '1' }, { set: 'subscriber', v: '3' }] },
    { login: 'sogfan', name: 'SogFan', color: '#7dffb2', badges: [{ set: 'vip', v: '1' }] },
    { login: 'streamelements', name: 'StreamElements', color: '#ffb02e', badges: [] },
    { login: 'someonenice', name: 'SomeoneNice', color: '', badges: [] },
  ];
  const DEMO_TEXT = ['hello chat Kappa', 'that was close LUL', 'this style looks great Kreygasm', 'love this stream <3',
    'what game is this?', 'gg', 'someone typed a message that gets deleted', 'check out !socials'];

  function demoEmotes(text) {
    const out = [];
    let pos = 0;
    for (const word of text.split(' ')) {
      if (DEMO_EMOTES[word]) out.push({ id: DEMO_EMOTES[word], start: pos, end: pos + word.length - 1 });
      pos += word.length + 1;
    }
    return out;
  }

  function startDemo() {
    let seq = 0;
    let n = 0;
    const post = () => {
      const user = DEMO_USERS[n % DEMO_USERS.length];
      const text = user.login === 'streamelements' ? 'Follow us on Twitter!' : DEMO_TEXT[n % DEMO_TEXT.length];
      const message = { seq: ++seq, ts: Date.now(), id: `demo-${seq}`, ...user, text, action: n % 7 === 6, emotes: demoEmotes(text) };
      latest = [...latest, message].slice(-50);
      n++;
      if (text.includes('deleted')) {
        window.setTimeout(() => { latest = latest.filter((m) => m.seq !== message.seq); sync(false); }, 4000);
      }
      sync(false);
    };
    applySettings(withOverride(structuredClone(DEFAULT_SETTINGS)));
    ensureExtras();
    for (let i = 0; i < 4; i++) post();
    window.setInterval(post, 1800);
  }

  if (demoMode) startDemo();
  else { dress(); void poll(); }
})();
