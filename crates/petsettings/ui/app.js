'use strict';

// 設定程式前端：只讀寫設定檔與送指令，所有規則（假日、排程）都在 Rust 那邊算。

const invoke = (cmd, args) => window.__TAURI__.core.invoke(cmd, args);

const WEEKDAYS = ['一', '二', '三', '四', '五', '六', '日']; // ISO：1 = 星期一
const REPEAT_KINDS = [['once', '單次'], ['daily', '每天'], ['weekly', '每週'], ['monthly', '每月'], ['workdays', '工作日'], ['interval', '定時']];
const HOLIDAY_POLICIES = [['normal', '照常'], ['skip', '跳過'], ['nextWorkday', '順延'], ['prevWorkday', '提前']];
const MONTHLY_KINDS = [['day', '第幾號'], ['lastDay', '月底'], ['nthWeekday', '第幾個星期幾']];
const NTH_LABELS = ['第一個', '第二個', '第三個', '第四個', '最後一個'];
const INTERVAL_CHOICES = [15, 30, 45, 60, 90, 120];
/** 時間到怎麼提醒（`petkit::reminders::Style`）。 */
const STYLE_CHOICES = [['card', '要按完成'], ['auto', '自己消失']];
const styleLabel = (style) => (STYLE_CHOICES.find(([v]) => v === style) ?? STYLE_CHOICES[0])[1];
/** 專注（番茄鐘）的時間選項（分鐘）與倒數顯示方式（`petkit::config::Countdown`）。 */
const FOCUS_CHOICES = [[15, '15 分'], [25, '25 分'], [30, '30 分'], [45, '45 分'], [50, '50 分']];
const SHORT_BREAK_CHOICES = [[3, '3 分'], [5, '5 分'], [10, '10 分']];
const LONG_BREAK_CHOICES = [[10, '10 分'], [15, '15 分'], [20, '20 分'], [30, '30 分']];
const COUNTDOWN_CHOICES = [['always', '一直顯示'], ['hover', '滑鼠移過去才顯示']];
/** 休息提醒：連續用電腦多久提醒一次（分鐘）。 */
const REST_CHOICES = [[30, '30 分'], [45, '45 分'], [50, '50 分'], [60, '1 小時'], [90, '1.5 小時']];
/** 新增提醒時一鍵套用：上班時間每隔一陣子提醒一下（自己消失，不用按）。`label` 是按鈕上的字，`title` 是套用後的提醒內容。 */
const TEMPLATES = [
  { label: '喝水', title: '喝水', everyMinutes: 60 },
  { label: '伸展', title: '起來伸展一下', everyMinutes: 90 },
  { label: '看遠方', title: '看看遠方，讓眼睛休息', everyMinutes: 30 },
];
const TEMPLATE_HOURS = ['09:00', '18:00'];
/** 桌寵的名字、牠怎麼叫你：都會放進講話泡泡，太長泡泡會塞不下。 */
const NAME_MAX = 20;
const NEVER_SLEEP_SECS = 365 * 24 * 3600;
const SLEEP_CHOICES = [[60, '1 分'], [180, '3 分'], [300, '5 分'], [600, '10 分'], [1800, '30 分'], [NEVER_SLEEP_SECS, '不睡']];
const SIZE_LABELS = ['小', '中', '大'];
const ACTIVITY_CHOICES = [['lively', '活潑'], ['normal', '普通'], ['calm', '安靜']];
const ROAM_CHOICES = [['screen', '整個螢幕'], ['inside', '只在範圍內'], ['avoid', '避開這些地方']];
const TALK_CHOICES = [['often', '常講'], ['sometimes', '偶爾'], ['never', '不講']];
/** 深淺色（`petkit::config::ColorScheme`）。 */
const COLOR_SCHEMES = [['system', '跟著系統'], ['light', '淺色'], ['dark', '深色']];
const DAY_KINDS = [['holiday', '放假'], ['workday', '上班']];
/** 動作速度：以規範預設為基準的倍率。 */
const SPEEDS = [['slow', '慢', 0.6], ['normal', '普通', 1], ['fast', '快', 1.5]];
const DEFAULT_VARIANT = 'default';
const PLAYER_TICK_MS = 50;
const TOAST_MS = 2200;
const CORE_POLL_MS = 3000;
const PREVIEW_DEBOUNCE_MS = 150;
const LAYOUT_REFRESH_MS = 600;
const UPDATE_POLL_MS = 500;
/** 核心還在查或下載，要繼續等結果。 */
const UPDATE_BUSY = ['checking', 'downloading'];
const DEFAULT_PAGE = 'reminders';
/** 功能開關：設定檔 features 的欄位 → 名稱與它的頁面（側邊欄「功能」那一組）。 */
const FEATURES = [['reminders', '提醒', 'reminders'], ['windowRestore', '視窗歸位', 'windows'], ['focus', '專注', 'focus']];

const state = {
  page: DEFAULT_PAGE,
  config: null,
  file: { reminders: [], anniversaries: [], dayOverrides: {} },
  /** 紀念日 id → `anniversary_info`（日期寫法、接下來幾次、還有幾天）。 */
  anniversaryInfo: {},
  /** 農曆月份、日子的寫法（`petkit::anniversary`）。 */
  lunarMonths: [],
  lunarDays: [],
  /** 從今天起官方日曆裡的國定假日、補班（`petkit::holidays::CalendarDay`）。 */
  officialDays: [],
  nextTimes: {},
  pets: [],
  sizes: [],
  version: '',
  coreRunning: false,
  /** 專注／休息中「停止專注」按鈕的文字，沒在專注是 null。 */
  focusStopLabel: null,
  windows: [],
  autostart: null,
  autostartError: '',
  /** 檢查更新的進度（`petkit::update_status::UpdateStatus`），沒按過「檢查更新」是 null。 */
  update: null,
};

// ---------- 小工具 ----------

function h(tag, props, ...children) {
  const el = document.createElement(tag);
  for (const [k, v] of Object.entries(props || {})) {
    if (k === 'class') el.className = v;
    else if (k.startsWith('on')) el.addEventListener(k.slice(2).toLowerCase(), v);
    else if (k === 'style') el.style.cssText = v;
    else if (k === 'value') el.value = v;
    else if (k === 'checked') el.checked = v;
    else if (v === true) el.setAttribute(k, '');
    else if (v !== false && v != null) el.setAttribute(k, v);
  }
  for (const c of children.flat()) {
    if (c == null || c === false) continue;
    el.append(c instanceof Node ? c : document.createTextNode(String(c)));
  }
  return el;
}

const icon = (name) => h('span', { class: 'icon', style: `--i:url(icons/${name}.svg)` });

function segmented(options, current, onPick) {
  return h('div', { class: 'segmented', role: 'group' },
    options.map(([value, label]) => h('button', { type: 'button', 'aria-pressed': String(value === current), onclick: () => onPick(value) }, label)));
}

/**
 * 刪不回來的操作先跳確認框：標題講要做什麼，`detail` 一行講後果，紅色那顆按鈕在右邊。
 * 焦點先放在「取消」，按 Esc 或點外面也是取消。回傳 true = 確定。
 */
function confirmDialog({ title, detail, confirm }) {
  return new Promise((resolve) => {
    const done = (ok) => {
      document.removeEventListener('keydown', onKey, true);
      backdrop.remove();
      resolve(ok);
    };
    const onKey = (e) => {
      if (e.key === 'Escape') {
        e.stopPropagation();
        done(false);
      }
    };
    const cancel = h('button', { class: 'btn btn-secondary', onclick: () => done(false) }, '取消');
    const dialog = h('div', { class: 'modal', role: 'alertdialog', 'aria-modal': 'true', 'aria-label': title, onclick: (e) => e.stopPropagation() },
      h('h2', {}, title),
      detail ? h('p', { class: 'modal-detail' }, detail) : null,
      h('div', { class: 'modal-foot' }, cancel, h('button', { class: 'btn btn-danger-solid', onclick: () => done(true) }, confirm)));
    const backdrop = h('div', { class: 'modal-backdrop', onclick: () => done(false) }, dialog);
    document.body.append(backdrop);
    document.addEventListener('keydown', onKey, true);
    cancel.focus();
  });
}

function toast(text) {
  const el = document.getElementById('toast');
  el.textContent = text;
  el.hidden = false;
  clearTimeout(toast.timer);
  toast.timer = setTimeout(() => { el.hidden = true; }, TOAST_MS);
}

const pad2 = (n) => String(n).padStart(2, '0');
const isoDate = (d) => `${d.getFullYear()}-${pad2(d.getMonth() + 1)}-${pad2(d.getDate())}`;
const parseDate = (s) => { const [y, m, d] = s.split('-').map(Number); return new Date(y, m - 1, d); };
const isoWeekday = (d) => ((d.getDay() + 6) % 7) + 1;
const shortDate = (s) => { const d = parseDate(s); return `${d.getMonth() + 1}/${d.getDate()}（${WEEKDAYS[isoWeekday(d) - 1]}）`; };

/** "2026-03-02T09:00:00" → 今天 09:00 / 明天 09:00 / 3/4（三）09:00 */
function friendlyWhen(at) {
  const [date, time] = at.split('T');
  const hm = time.slice(0, 5);
  const today = new Date();
  const tomorrow = new Date(today.getFullYear(), today.getMonth(), today.getDate() + 1);
  if (date === isoDate(today)) return `今天 ${hm}`;
  if (date === isoDate(tomorrow)) return `明天 ${hm}`;
  return `${shortDate(date)} ${hm}`;
}

function repeatSummary(r) {
  const rep = r.repeat;
  switch (rep.kind) {
    case 'once': return `單次 · ${shortDate(r.startDate)}`;
    case 'daily': return '每天';
    case 'weekly': return `每週${rep.days.slice().sort().map((d) => WEEKDAYS[d - 1]).join('、')}`;
    case 'monthly': {
      const rule = rep.rule;
      if (rule.kind === 'day') return `每月 ${rule.day} 號`;
      if (rule.kind === 'lastDay') return '每月月底';
      return `每月${NTH_LABELS[rule.nth - 1]}星期${WEEKDAYS[rule.weekday - 1]}`;
    }
    case 'workdays': return '工作日';
    case 'interval': return `${r.time}～${rep.until} 每 ${rep.everyMinutes} 分鐘${rep.workdaysOnly ? ' · 上班日' : ''}`;
    default: return '';
  }
}

/** 自己就照上班日走的規則，沒有「遇到假日」這個選項。 */
const usesHolidayPolicy = (r) => !(r.repeat.kind === 'workdays' || (r.repeat.kind === 'interval' && r.repeat.workdaysOnly));
const holidayLabel = (p) => (HOLIDAY_POLICIES.find(([v]) => v === p) || [])[1];

// ---------- 主題（design/tokens.json → CSS 變數） ----------

function applyTokens(tokens) {
  const kebab = (s) => s.replace(/[A-Z]/g, (c) => `-${c.toLowerCase()}`);
  const palette = (p) => Object.entries(p).map(([k, v]) => `--color-${kebab(k)}:${v};`).join('');
  const flat = [];
  const walk = (obj, prefix) => {
    for (const [k, v] of Object.entries(obj)) {
      const name = `${prefix}-${kebab(k)}`;
      if (Array.isArray(v)) flat.push(`${name}:${v.join(',')};`);
      else if (typeof v === 'object') walk(v, name);
      else flat.push(`${name}:${v};`);
    }
  };
  for (const [section, value] of Object.entries(tokens)) if (section !== 'color') walk(value, `--${section}`);
  // 深淺色看 <html data-theme>（設定的 colorScheme）：system 跟著系統，light／dark 固定。
  const dark = `color-scheme:dark;${palette(tokens.color.dark)}`;
  const css = `:root{color-scheme:light;${palette(tokens.color.light)}${flat.join('')}}`
    + `@media (prefers-color-scheme: dark){:root:not([data-theme="light"]):not([data-theme="dark"]){${dark}}}`
    + `:root[data-theme="dark"]{${dark}}`;
  document.head.append(h('style', {}, css));
}

// ---------- 資料 ----------

async function loadReminders() {
  [state.file, state.officialDays] = await Promise.all([invoke('load_reminders'), invoke('official_days')]);
  state.file.dayOverrides = state.file.dayOverrides || {};
  state.file.anniversaries = state.file.anniversaries || [];
  await refreshNextTimes();
}

async function refreshNextTimes() {
  const entries = await Promise.all(state.file.reminders.map(async (r) => {
    const items = await invoke('preview', { reminder: r, overrides: state.file.dayOverrides });
    const fire = items.find((i) => i.kind === 'fire');
    return [r.id, fire ? fire.at : null];
  }));
  state.nextTimes = Object.fromEntries(entries);
  const infos = await Promise.all(state.file.anniversaries.map(async (a) => [a.id, await invoke('anniversary_info', { anniversary: a })]));
  state.anniversaryInfo = Object.fromEntries(infos);
}

async function saveReminders() {
  await invoke('save_reminders', { file: state.file });
  await refreshNextTimes();
}

async function saveConfig() {
  await invoke('save_config', { config: state.config });
}

/** 設定列：左邊標題（可帶圖示與一行小字），右邊控制項。 */
function settingRow(title, control, desc) {
  return h('div', { class: 'setting' },
    h('div', { class: 'setting-main' }, h('div', { class: 'setting-title' }, title), desc ? h('div', { class: 'setting-desc' }, desc) : null),
    control);
}

/** 表單欄位：標籤永遠在上面（不拿 placeholder 當標籤）。 */
function field(label, control, id) {
  return h('div', { class: 'field' }, h('label', { for: id }, label), control);
}

/** 直接改設定檔某個欄位的開關。 */
function configSwitch(key, label) {
  return h('label', { class: 'switch' },
    h('input', { type: 'checkbox', checked: state.config[key], 'aria-label': label, onchange: async (e) => { state.config[key] = e.target.checked; await saveConfig(); render(); } }),
    h('span'));
}

async function setFeature(key, on) {
  state.config.features[key] = on;
  await saveConfig();
  render();
}

/** 功能頁的標題：名稱旁邊就是這個功能的開關。 */
function featureTitle(key) {
  const [, name] = FEATURES.find(([k]) => k === key);
  return h('div', { class: 'row' },
    h('h1', {}, name),
    h('label', { class: 'switch' },
      h('input', { type: 'checkbox', checked: state.config.features[key], 'aria-label': name, onchange: (e) => setFeature(key, e.target.checked) }),
      h('span')));
}

/** 功能關著時，那一頁只留標題（含開關）。 */
function featureOff(page, key) {
  if (state.config.features[key]) return false;
  const [, name] = FEATURES.find(([k]) => k === key);
  page.append(h('div', { class: 'page-head' }, featureTitle(key)));
  page.append(h('div', { class: 'card empty' },
    h('img', { src: currentPetImage(), alt: '' }),
    h('div', { class: 'empty-title' }, `${name}已關閉`)));
  return true;
}

// ---------- 頁面：提醒 ----------

function renderReminders(page) {
  if (featureOff(page, 'reminders')) return;
  const sorted = state.file.reminders.slice().sort((a, b) => {
    const ta = state.nextTimes[a.id] || '9999', tb = state.nextTimes[b.id] || '9999';
    return (a.enabled === b.enabled ? 0 : a.enabled ? -1 : 1) || ta.localeCompare(tb);
  });
  page.append(h('div', { class: 'page-head' },
    featureTitle('reminders'),
    h('button', { class: 'btn btn-primary', onclick: () => openEditor(null) }, icon('plus'), '新增提醒')));

  if (!sorted.length) {
    page.append(h('div', { class: 'card empty' },
      h('img', { src: currentPetImage(), alt: '' }),
      h('div', { class: 'empty-title' }, '還沒有提醒'),
      h('button', { class: 'btn btn-primary', onclick: () => openEditor(null) }, icon('plus'), '新增提醒')));
  } else {
    page.append(h('div', { class: 'card list' }, sorted.map(reminderRow)));
  }
  page.append(renderAnniversaries());
  page.append(h('section', { class: 'section' },
    h('h2', {}, '提醒方式'),
    h('div', { class: 'card' },
      settingRow('提醒音', configSwitch('reminderSound', '提醒音')),
      settingRow('全螢幕時照樣跳出來', configSwitch('remindInFullscreen', '全螢幕時照樣跳出來')),
      settingRow('提醒卡', h('button', { class: 'btn btn-secondary btn-small', disabled: !state.coreRunning, onclick: () => invoke('send_core', { command: { cmd: 'testReminder', title: '這是提醒卡的樣子' } }) }, icon('play'), '看看樣子')))));
  const setRest = async (key, value) => { state.config[key] = value; await saveConfig(); render(); };
  page.append(h('section', { class: 'section' },
    h('h2', {}, '休息提醒'),
    h('div', { class: 'card' },
      settingRow('連續用電腦太久提醒起來動一動', configSwitch('restReminder', '休息提醒')),
      state.config.restReminder ? settingRow('連續用多久', segmented(REST_CHOICES, state.config.restEveryMinutes, (v) => setRest('restEveryMinutes', v))) : null,
      state.config.restReminder ? settingRow('卡片', segmented(STYLE_CHOICES, state.config.restStyle, (v) => setRest('restStyle', v))) : null)));
  page.append(renderOfficialDays(), renderExtraDays());
}

// ---------- 紀念日 ----------
// 每年同一天（國曆或農曆），一筆可以有好幾個「提前幾天、幾點」的提醒；卡片一定要按完成才收掉。

/** 提前幾天的選項（0 = 當天）。 */
const DAYS_BEFORE_CHOICES = [0, 1, 2, 3, 5, 7, 14, 30];
/** 一筆紀念日最多幾個提醒。 */
const ALERTS_MAX = 5;
const NEW_ALERTS = [{ daysBefore: 3, time: '09:00' }, { daysBefore: 0, time: '09:00' }];
/** 國曆每個月最多幾天（2 月給 29，平年那天照 2/28 過）。 */
const SOLAR_MONTH_DAYS = [31, 29, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
const LUNAR_MONTH_DAYS = 30;
const ANNIVERSARY_TITLE_MAX = 30;

const daysBeforeLabel = (n) => (n === 0 ? '當天' : n === 1 ? '前一天' : `${n} 天前`);

/** 「3 天前、當天 09:00」；時間不一樣就各寫各的。 */
function alertsSummary(alerts) {
  const sorted = alerts.slice().sort((a, b) => b.daysBefore - a.daysBefore);
  const sameTime = sorted.every((a) => a.time === sorted[0].time);
  return sameTime
    ? `${sorted.map((a) => daysBeforeLabel(a.daysBefore)).join('、')} ${sorted[0]?.time ?? ''}`
    : sorted.map((a) => `${daysBeforeLabel(a.daysBefore)} ${a.time}`).join('、');
}

/** 「今天」「明天」「12 天後」的那顆。 */
function countdown(daysLeft) {
  if (daysLeft == null) return null;
  if (daysLeft <= 1) return h('div', { class: 'countdown soon' }, h('span', { class: 'countdown-num' }, daysLeft === 0 ? '今天' : '明天'));
  return h('div', { class: 'countdown' }, h('span', { class: 'countdown-num' }, String(daysLeft)), h('span', { class: 'countdown-unit' }, '天後'));
}

function renderAnniversaries() {
  const list = state.file.anniversaries.slice().sort((a, b) => {
    const da = state.anniversaryInfo[a.id]?.daysLeft ?? Infinity, db = state.anniversaryInfo[b.id]?.daysLeft ?? Infinity;
    return (a.enabled === b.enabled ? 0 : a.enabled ? -1 : 1) || da - db;
  });
  return h('section', { class: 'section' },
    h('div', { class: 'section-head' },
      h('h2', {}, '紀念日'),
      h('button', { class: 'btn btn-secondary btn-small', onclick: () => openAnniversary(null) }, icon('plus'), '新增紀念日')),
    h('div', { class: 'card list' }, list.length
      ? list.map(anniversaryRow)
      : h('div', { class: 'list-row list-note' }, icon('cake'), '記下生日、紀念日（農曆也可以），時間到桌寵會提醒你')));
}

function anniversaryRow(a) {
  const info = state.anniversaryInfo[a.id];
  const next = info?.upcoming[0];
  const date = next ? parseDate(next.date) : null;
  const lunar = a.date.calendar === 'lunar';
  const toggle = h('label', { class: 'switch', onclick: (e) => e.stopPropagation(), title: a.enabled ? '暫停這個紀念日' : '恢復這個紀念日' },
    h('input', { type: 'checkbox', checked: a.enabled, 'aria-label': `${a.title} 啟用`, onchange: async (e) => { a.enabled = e.target.checked; await saveReminders(); render(); } }),
    h('span'));
  return h('div', { class: `list-row clickable day-row${a.enabled ? '' : ' off'}`, onclick: () => openAnniversary(a) },
    date
      ? h('div', { class: 'date-tile', 'aria-hidden': 'true' }, h('span', { class: 'date-tile-month' }, `${date.getMonth() + 1} 月`), h('span', { class: 'date-tile-day' }, String(date.getDate())))
      : h('span', { class: 'row-dot' }, icon('cake')),
    h('div', { class: 'row-main' },
      h('div', { class: 'row-title', title: a.title }, a.title),
      h('div', { class: 'row-meta' },
        lunar && info ? h('span', { class: 'tag tag-accent' }, info.label) : null,
        next?.nth > 0 ? h('span', { class: 'tag' }, `滿 ${next.nth} 年`) : null,
        a.alerts.length ? h('span', { class: 'tag' }, icon('bell'), alertsSummary(a.alerts)) : null)),
    a.enabled ? countdown(info?.daysLeft) : null,
    toggle);
}

function newAnniversary() {
  const today = new Date();
  return {
    id: (crypto.randomUUID && crypto.randomUUID()) || String(Date.now()),
    title: '',
    enabled: true,
    date: { calendar: 'solar', month: today.getMonth() + 1, day: today.getDate() },
    year: null,
    alerts: structuredClone(NEW_ALERTS),
  };
}

function openAnniversary(existing) {
  const ed = { draft: structuredClone(existing || newAnniversary()), isNew: !existing, info: null };
  openDrawer((drawer) => drawAnniversary(drawer, ed));
  refreshAnniversaryPreview(ed);
  setTimeout(() => document.getElementById('anniv-title')?.focus(), 50);
}

const anniversaryValid = (a) => a.title.trim() && a.alerts.length > 0 && a.alerts.every((x) => x.time);

function refreshAnniversaryPreview(ed) {
  clearTimeout(refreshAnniversaryPreview.timer);
  refreshAnniversaryPreview.timer = setTimeout(async () => {
    ed.info = await invoke('anniversary_info', { anniversary: ed.draft });
    drawAnniversaryPreview(ed);
  }, PREVIEW_DEBOUNCE_MS);
}

function drawAnniversaryPreview(ed) {
  const box = document.getElementById('anniv-preview');
  if (!box || !ed.info) return;
  box.replaceChildren(...ed.info.upcoming.map((o, i) => {
    const d = parseDate(o.date);
    return h('div', { class: 'preview-row' },
      h('span', { class: 'when' }, `${d.getFullYear()}/${d.getMonth() + 1}/${d.getDate()}（${WEEKDAYS[isoWeekday(d) - 1]}）`),
      o.nth > 0 ? h('span', { class: 'tag' }, `滿 ${o.nth} 年`) : null,
      i === 0 && ed.info.daysLeft != null ? h('span', { class: 'tag tag-accent' }, ed.info.daysLeft === 0 ? '今天' : `${ed.info.daysLeft} 天後`) : null);
  }));
}

function drawAnniversary(drawer, ed) {
  const a = ed.draft;
  const lunar = a.date.calendar === 'lunar';
  const redraw = () => { drawAnniversary(drawer, ed); refreshAnniversaryPreview(ed); };
  const saveBtn = () => document.getElementById('anniv-save');
  const revalidate = () => { saveBtn().disabled = !anniversaryValid(a); };

  const monthNames = lunar ? state.lunarMonths : Array.from({ length: 12 }, (_, i) => `${i + 1} 月`);
  const dayCount = lunar ? LUNAR_MONTH_DAYS : SOLAR_MONTH_DAYS[a.date.month - 1];
  const dayName = (d) => (lunar ? state.lunarDays[d - 1] : `${d} 日`);
  const monthSelect = h('select', { class: 'select', 'aria-label': '月', onchange: (e) => {
    a.date.month = Number(e.target.value);
    if (!lunar) a.date.day = Math.min(a.date.day, SOLAR_MONTH_DAYS[a.date.month - 1]);
    redraw();
  } }, monthNames.map((name, i) => h('option', { value: i + 1, selected: a.date.month === i + 1 }, name)));
  const daySelect = h('select', { class: 'select', 'aria-label': '日', onchange: (e) => { a.date.day = Number(e.target.value); redraw(); } },
    Array.from({ length: dayCount }, (_, i) => h('option', { value: i + 1, selected: a.date.day === i + 1 }, dayName(i + 1))));

  const alertRow = (alert, i) => h('div', { class: 'alert-row' },
    h('select', { class: 'select', 'aria-label': `第 ${i + 1} 個提醒提前幾天`, onchange: (e) => { alert.daysBefore = Number(e.target.value); refreshAnniversaryPreview(ed); } },
      DAYS_BEFORE_CHOICES.map((n) => h('option', { value: n, selected: alert.daysBefore === n }, daysBeforeLabel(n)))),
    h('input', { class: 'input input-time', type: 'time', value: alert.time, 'aria-label': `第 ${i + 1} 個提醒幾點`, onchange: (e) => { alert.time = e.target.value; revalidate(); } }),
    h('button', { type: 'button', class: 'btn btn-ghost btn-small', 'aria-label': `刪掉第 ${i + 1} 個提醒`, disabled: a.alerts.length === 1, onclick: () => { a.alerts.splice(i, 1); redraw(); } }, icon('x')));

  const deleteBtn = ed.isNew ? null : h('button', {
    class: 'btn btn-danger',
    onclick: async () => {
      if (!await confirmDialog({ title: `刪除「${a.title}」？`, confirm: '刪除' })) return;
      state.file.anniversaries = state.file.anniversaries.filter((x) => x.id !== a.id);
      await saveReminders();
      closeEditor();
      toast('已刪除');
      render();
    },
  }, icon('trash-2'), '刪除');

  const scrolled = drawer.querySelector('.drawer-body')?.scrollTop ?? 0;
  drawer.replaceChildren(
    h('div', { class: 'drawer-head' },
      h('h2', {}, ed.isNew ? '新增紀念日' : '編輯紀念日'),
      h('button', { class: 'btn btn-ghost btn-small', 'aria-label': '關閉', onclick: closeEditor }, icon('x'))),
    h('div', { class: 'drawer-body' },
      field('名稱', h('input', { id: 'anniv-title', class: 'input', placeholder: '例：媽媽生日、結婚紀念日', value: a.title, maxlength: ANNIVERSARY_TITLE_MAX,
        oninput: (e) => { a.title = e.target.value; revalidate(); } }), 'anniv-title'),
      h('div', { class: 'field' }, h('span', { class: 'field-label' }, '日期'),
        segmented([['solar', '國曆'], ['lunar', '農曆']], a.date.calendar, (c) => {
          a.date = c === 'lunar' ? { calendar: c, month: a.date.month, day: Math.min(a.date.day, LUNAR_MONTH_DAYS), leap: false } : { calendar: c, month: a.date.month, day: Math.min(a.date.day, SOLAR_MONTH_DAYS[a.date.month - 1]) };
          redraw();
        }),
        h('div', { class: 'row' }, monthSelect, daySelect,
          lunar ? h('label', { class: 'check' }, h('input', { type: 'checkbox', checked: !!a.date.leap, onchange: (e) => { a.date.leap = e.target.checked; redraw(); } }), '閏月') : null)),
      field('哪一年（選填）', h('input', { id: 'anniv-year', class: 'input input-number', type: 'number', min: 1901, max: 2100, placeholder: '例：1966', value: a.year ?? '',
        onchange: (e) => { const y = Number(e.target.value); a.year = e.target.value && y >= 1901 && y <= 2100 ? y : null; refreshAnniversaryPreview(ed); } }), 'anniv-year'),
      h('div', { class: 'field' }, h('span', { class: 'field-label' }, '提醒'),
        h('div', { class: 'alert-list' }, a.alerts.map(alertRow)),
        a.alerts.length < ALERTS_MAX ? h('div', {}, h('button', { type: 'button', class: 'btn btn-ghost btn-small', onclick: () => {
          const used = new Set(a.alerts.map((x) => x.daysBefore));
          a.alerts.push({ daysBefore: DAYS_BEFORE_CHOICES.find((n) => !used.has(n)) ?? 0, time: a.alerts.at(-1)?.time ?? '09:00' });
          redraw();
        } }, icon('plus'), '再加一個提醒')) : null),
      h('div', { class: 'field' }, h('span', { class: 'field-label' }, '接下來'), h('div', { class: 'preview', id: 'anniv-preview' }))),
    h('div', { class: 'drawer-foot' },
      deleteBtn,
      h('span', { class: 'spacer' }),
      h('button', { class: 'btn btn-ghost', onclick: closeEditor }, '取消'),
      h('button', { id: 'anniv-save', class: 'btn btn-primary', disabled: !anniversaryValid(a), onclick: async () => {
        a.title = a.title.trim();
        const i = state.file.anniversaries.findIndex((x) => x.id === a.id);
        if (i >= 0) state.file.anniversaries[i] = a; else state.file.anniversaries.push(a);
        await saveReminders();
        closeEditor();
        toast('已儲存');
        render();
      } }, icon('check'), '儲存')));
  drawAnniversaryPreview(ed);
  drawer.querySelector('.drawer-body').scrollTop = scrolled;
}

// ---------- 國定假日與補班 ----------
// 開著 = 照官方日曆；關掉 = 在 dayOverrides 寫成平常那樣（國定假日照常上班、補班照常放假）。

const opposite = (kind) => (kind === 'holiday' ? 'workday' : 'holiday');
const officialOn = (d) => (state.file.dayOverrides[d.date] ?? d.kind) === d.kind;
const isOfficialDate = (date) => state.officialDays.some((d) => d.date === date);

async function setOfficialDays(days, on) {
  for (const d of days) {
    if (on) delete state.file.dayOverrides[d.date];
    else state.file.dayOverrides[d.date] = opposite(d.kind);
  }
  await saveReminders();
  render();
}

function renderOfficialDays() {
  const days = state.officialDays;
  const onCount = days.filter(officialOn).length;
  const byYear = Map.groupBy(days, (d) => d.date.slice(0, 4));
  return h('section', { class: 'section' },
    h('div', { class: 'section-head' },
      h('h2', {}, '國定假日與補班'),
      days.length ? h('div', { class: 'section-actions' },
        h('button', { class: 'btn btn-ghost btn-small', disabled: onCount === days.length, onclick: () => setOfficialDays(days, true) }, icon('check'), '全部開啟'),
        h('button', { class: 'btn btn-ghost btn-small', disabled: onCount === 0, onclick: () => setOfficialDays(days, false) }, icon('x'), '全部關閉')) : null),
    days.length
      ? h('div', { class: 'card list' }, [...byYear].flatMap(([year, list]) => [h('div', { class: 'list-year' }, `${year} 年`), ...list.map(officialRow)]))
      : h('div', { class: 'card list' }, h('div', { class: 'list-row list-note' }, icon('calendar-off'), '接下來沒有國定假日，新的行事曆公布後會自動出現')));
}

function officialRow(d) {
  const on = officialOn(d);
  const holiday = d.kind === 'holiday';
  const date = parseDate(d.date);
  const name = d.note || (holiday ? '放假' : '補班');
  return h('div', { class: `list-row day-row official-row${on ? '' : ' off'}` },
    h('div', { class: `date-tile${holiday ? '' : ' work'}`, 'aria-hidden': 'true' },
      h('span', { class: 'date-tile-month' }, `${date.getMonth() + 1} 月`),
      h('span', { class: 'date-tile-day' }, String(date.getDate()))),
    h('div', { class: 'row-main' },
      h('div', { class: 'row-title', title: name }, name),
      h('div', { class: 'row-meta' },
        h('span', {}, `星期${WEEKDAYS[isoWeekday(date) - 1]}`),
        on
          ? h('span', { class: `tag${holiday ? ' tag-accent' : ''}` }, holiday ? '放假' : '補班')
          : h('span', { class: 'tag tag-off' }, holiday ? '照常上班' : '照常放假'))),
    h('label', { class: 'switch' },
      h('input', { type: 'checkbox', checked: on, 'aria-label': `${shortDate(d.date)} ${name}`, onchange: (e) => setOfficialDays([d], e.target.checked) }),
      h('span')));
}

function reminderRow(r) {
  const next = state.nextTimes[r.id];
  const toggle = h('label', { class: 'switch', onclick: (e) => e.stopPropagation(), title: r.enabled ? '暫停這則提醒' : '恢復這則提醒' },
    h('input', { type: 'checkbox', checked: r.enabled, 'aria-label': `${r.title} 啟用`, onchange: async (e) => { r.enabled = e.target.checked; await saveReminders(); render(); } }),
    h('span'));
  return h('div', { class: `list-row clickable${r.enabled ? '' : ' off'}`, onclick: () => openEditor(r) },
    h('span', { class: `row-dot${r.enabled ? '' : ' muted'}`, title: styleLabel(r.style) }, icon(r.style === 'auto' ? 'message-dots' : 'bell')),
    h('div', { class: 'row-main' },
      h('div', { class: 'row-when' }, r.enabled ? (next ? friendlyWhen(next) : '不會再響') : '已暫停'),
      h('div', { class: 'row-title' }, r.title),
      h('div', { class: 'row-meta' },
        h('span', { class: 'tag' }, icon('clock'), repeatSummary(r)),
        usesHolidayPolicy(r) && r.holiday !== 'normal' ? h('span', { class: 'tag tag-accent' }, icon('calendar-off'), `遇假日${holidayLabel(r.holiday)}`) : null)),
    toggle);
}

function renderExtraDays() {
  // 國定假日、補班那幾天在上面那塊開關，這裡只列使用者自己加的額外上班／放假日。
  const entries = Object.entries(state.file.dayOverrides).filter(([date]) => !isOfficialDate(date)).sort(([a], [b]) => a.localeCompare(b));
  const draft = { date: isoDate(new Date()), kind: 'holiday' };
  const kindPicker = h('div');
  const drawKind = () => { kindPicker.replaceChildren(segmented(DAY_KINDS, draft.kind, (v) => { draft.kind = v; drawKind(); })); };
  drawKind();
  const dateInput = h('input', { class: 'input', type: 'date', value: draft.date, 'aria-label': '日期', onchange: (e) => { draft.date = e.target.value; } });
  return h('section', { class: 'section' },
    h('h2', {}, '額外上班／放假日'),
    h('div', { class: 'card list' },
      entries.map(([date, kind]) => h('div', { class: 'list-row' },
        h('div', { class: 'row-main' }, h('div', { class: 'row-title' }, shortDate(date))),
        h('span', { class: `tag${kind === 'holiday' ? ' tag-accent' : ''}` }, (DAY_KINDS.find(([v]) => v === kind) || [])[1]),
        h('button', { class: 'btn btn-ghost btn-small', 'aria-label': `移除 ${shortDate(date)}`, onclick: async () => { delete state.file.dayOverrides[date]; await saveReminders(); render(); } }, icon('x')))),
      h('div', { class: 'list-row' },
        h('div', { class: 'row' }, dateInput, kindPicker),
        h('div', { style: 'flex:1' }),
        h('button', { class: 'btn btn-secondary btn-small', onclick: async () => {
          if (!draft.date) return;
          state.file.dayOverrides[draft.date] = draft.kind;
          await saveReminders();
          toast('已加入');
          render();
        } }, icon('plus'), '加入'))));
}

// ---------- 提醒編輯面板 ----------

function newReminder() {
  const now = new Date();
  return {
    id: (crypto.randomUUID && crypto.randomUUID()) || String(Date.now()),
    title: '',
    enabled: true,
    time: `${pad2((now.getHours() + 1) % 24)}:00`,
    repeat: { kind: 'daily' },
    holiday: 'normal',
    startDate: isoDate(now),
    style: 'card',
  };
}

function repeatDefaults(kind, r) {
  const today = parseDate(r.startDate);
  switch (kind) {
    case 'weekly': return { kind, days: [isoWeekday(today)] };
    case 'monthly': return { kind, rule: { kind: 'day', day: today.getDate() } };
    case 'interval': return { kind, everyMinutes: 60, until: '18:00', workdaysOnly: true };
    default: return { kind };
  }
}

let editor = null;

function openEditor(existing) {
  editor = { draft: structuredClone(existing || newReminder()), isNew: !existing, preview: [] };
  document.getElementById('drawer-backdrop').hidden = false;
  const drawer = document.getElementById('drawer');
  drawer.setAttribute('aria-hidden', 'false');
  drawEditor();
  requestAnimationFrame(() => drawer.classList.add('open'));
  updatePreview();
  setTimeout(() => drawer.querySelector('#title')?.focus(), 50);
}

function closeEditor() {
  const drawer = document.getElementById('drawer');
  drawer.classList.remove('open');
  drawer.setAttribute('aria-hidden', 'true');
  document.getElementById('drawer-backdrop').hidden = true;
  editor = null;
}

function updatePreview() {
  clearTimeout(updatePreview.timer);
  updatePreview.timer = setTimeout(async () => {
    if (!editor) return;
    editor.preview = await invoke('preview', { reminder: editor.draft, overrides: state.file.dayOverrides });
    drawPreview();
  }, PREVIEW_DEBOUNCE_MS);
}

function changed() {
  drawEditor();
  updatePreview();
}

/** 打字型的欄位（時間、日期）改了：不重畫抽屜 —— 重畫會把輸入框換掉，打到一半的數字就跳開了（打 30 變 03）。 */
function edited() {
  document.getElementById('save-btn').disabled = !editorValid(editor.draft);
  updatePreview();
}

function editorValid(r) {
  return r.title.trim() && r.time && !(r.repeat.kind === 'weekly' && !r.repeat.days.length);
}

function drawEditor() {
  const r = editor.draft;
  const drawer = document.getElementById('drawer');

  const repeatExtra = (() => {
    const rep = r.repeat;
    if (rep.kind === 'once') {
      return field('日期', h('input', { id: 'once-date', class: 'input', type: 'date', value: r.startDate, onchange: (e) => { r.startDate = e.target.value; edited(); } }), 'once-date');
    }
    if (rep.kind === 'weekly') {
      return h('div', { class: 'field' }, h('span', { class: 'field-label' }, '星期'),
        h('div', { class: 'chips' }, WEEKDAYS.map((label, i) => {
          const day = i + 1;
          const on = rep.days.includes(day);
          return h('button', { type: 'button', class: 'chip', 'aria-pressed': String(on), 'aria-label': `星期${label}`, onclick: () => {
            rep.days = on ? rep.days.filter((d) => d !== day) : [...rep.days, day].sort();
            changed();
          } }, label);
        })));
    }
    if (rep.kind === 'monthly') {
      const rule = rep.rule;
      const daySelect = h('select', { class: 'select', 'aria-label': '幾號', onchange: (e) => { rule.day = Number(e.target.value); changed(); } },
        Array.from({ length: 31 }, (_, i) => h('option', { value: i + 1, selected: rule.day === i + 1 }, `${i + 1} 號`)));
      const nthSelect = h('select', { class: 'select', 'aria-label': '第幾個', onchange: (e) => { rule.nth = Number(e.target.value); changed(); } },
        NTH_LABELS.map((label, i) => h('option', { value: i + 1, selected: rule.nth === i + 1 }, label)));
      const wdSelect = h('select', { class: 'select', 'aria-label': '星期幾', onchange: (e) => { rule.weekday = Number(e.target.value); changed(); } },
        WEEKDAYS.map((label, i) => h('option', { value: i + 1, selected: rule.weekday === i + 1 }, `星期${label}`)));
      return h('div', { class: 'field' }, h('span', { class: 'field-label' }, '哪一天'),
        segmented(MONTHLY_KINDS, rule.kind, (k) => {
          const today = parseDate(r.startDate);
          rep.rule = k === 'day' ? { kind: k, day: today.getDate() } : k === 'lastDay' ? { kind: k } : { kind: k, nth: Math.min(5, Math.ceil(today.getDate() / 7)), weekday: isoWeekday(today) };
          changed();
        }),
        rule.kind === 'day' ? h('div', { class: 'row' }, daySelect) : null,
        rule.kind === 'nthWeekday' ? h('div', { class: 'row' }, nthSelect, wdSelect) : null);
    }
    if (rep.kind === 'interval') {
      // 開始、結束、間隔放在同一塊：上面那格「時間」選定時時不出現，開始時間就在這裡。
      return h('div', { class: 'field' }, h('span', { class: 'field-label' }, '時段'),
        h('div', { class: 'row' },
          '從',
          h('input', { id: 'time', class: 'input input-time', type: 'time', value: r.time, 'aria-label': '從幾點', onchange: (e) => { r.time = e.target.value; edited(); } }),
          '到',
          h('input', { class: 'input input-time', type: 'time', value: rep.until, 'aria-label': '到幾點', onchange: (e) => { rep.until = e.target.value; edited(); } })),
        h('div', { class: 'row' },
          '每',
          h('select', { class: 'select', 'aria-label': '每幾分鐘', onchange: (e) => { rep.everyMinutes = Number(e.target.value); changed(); } },
            INTERVAL_CHOICES.map((m) => h('option', { value: m, selected: rep.everyMinutes === m }, `${m} 分鐘`))),
          '一次'),
        h('label', { class: 'check' }, h('input', { type: 'checkbox', checked: rep.workdaysOnly, onchange: (e) => { rep.workdaysOnly = e.target.checked; changed(); } }), '只在上班日'));
    }
    return null;
  })();

  const deleteBtn = editor.isNew ? null : h('button', {
    class: 'btn btn-danger',
    onclick: async () => {
      if (!await confirmDialog({ title: `刪除「${r.title}」？`, confirm: '刪除' })) return;
      state.file.reminders = state.file.reminders.filter((x) => x.id !== r.id);
      await saveReminders();
      closeEditor();
      toast('已刪除');
      render();
    },
  }, icon('trash-2'), '刪除');

  // 選了切換鈕整個抽屜會重畫：記住捲到哪，畫完捲回去，不然會跳回最上面。
  const scrolled = drawer.querySelector('.drawer-body')?.scrollTop ?? 0;
  drawer.replaceChildren(
    h('div', { class: 'drawer-head' },
      h('h2', {}, editor.isNew ? '新增提醒' : '編輯提醒'),
      h('button', { class: 'btn btn-ghost btn-small', 'aria-label': '關閉', onclick: closeEditor }, icon('x'))),
    h('div', { class: 'drawer-body' },
      editor.isNew ? h('div', { class: 'field' }, h('span', { class: 'field-label' }, '常用'),
        h('div', { class: 'row' }, TEMPLATES.map((t) => h('button', { type: 'button', class: 'btn btn-secondary btn-small', onclick: () => {
          r.title = t.title;
          r.time = TEMPLATE_HOURS[0];
          r.repeat = { kind: 'interval', everyMinutes: t.everyMinutes, until: TEMPLATE_HOURS[1], workdaysOnly: true };
          r.style = 'auto';
          changed();
        } }, t.label)))) : null,
      field('提醒內容', h('input', { id: 'title', class: 'input', placeholder: '例：喝水、繳卡費', value: r.title, maxlength: 60,
        oninput: (e) => { r.title = e.target.value; document.getElementById('save-btn').disabled = !editorValid(r); } }), 'title'),
      r.repeat.kind === 'interval' ? null : field('時間', h('input', { id: 'time', class: 'input input-time', type: 'time', value: r.time, onchange: (e) => { r.time = e.target.value; edited(); } }), 'time'),
      h('div', { class: 'field' }, h('span', { class: 'field-label' }, '重複'),
        segmented(REPEAT_KINDS, r.repeat.kind, (k) => { r.repeat = repeatDefaults(k, r); changed(); })),
      repeatExtra,
      usesHolidayPolicy(r) ? h('div', { class: 'field' }, h('span', { class: 'field-label' }, '遇到假日'),
        segmented(HOLIDAY_POLICIES, r.holiday, (p) => { r.holiday = p; changed(); })) : null,
      h('div', { class: 'field' }, h('span', { class: 'field-label' }, '卡片'),
        segmented(STYLE_CHOICES, r.style || 'card', (s) => { r.style = s; changed(); })),
      h('div', { class: 'field' }, h('span', { class: 'field-label' }, '接下來'), h('div', { class: 'preview', id: 'preview' }))),
    h('div', { class: 'drawer-foot' },
      deleteBtn,
      h('span', { class: 'spacer' }),
      h('button', { class: 'btn btn-ghost', onclick: closeEditor }, '取消'),
      h('button', { id: 'save-btn', class: 'btn btn-primary', disabled: !editorValid(r), onclick: async () => {
        r.title = r.title.trim();
        const i = state.file.reminders.findIndex((x) => x.id === r.id);
        if (i >= 0) state.file.reminders[i] = r; else state.file.reminders.push(r);
        await saveReminders();
        closeEditor();
        toast('已儲存');
        render();
      } }, icon('check'), '儲存')));
  drawPreview();
  drawer.querySelector('.drawer-body').scrollTop = scrolled;
}

function drawPreview() {
  const box = document.getElementById('preview');
  if (!box || !editor) return;
  if (!editor.preview.length) {
    box.replaceChildren(h('div', { class: 'preview-row' }, h('span', { class: 'when' }, '不會再響')));
    return;
  }
  box.replaceChildren(...editor.preview.map((item) => {
    if (item.kind === 'skipped') {
      return h('div', { class: 'preview-row skipped' }, h('span', { class: 'when' }, shortDate(item.date)), h('span', { class: 'tag' }, item.note ? `${item.note} · 跳過` : '假日 · 跳過'));
    }
    return h('div', { class: 'preview-row' },
      h('span', { class: 'when' }, friendlyWhen(item.at)),
      item.movedFrom ? h('span', { class: 'tag tag-accent' }, `${item.note || '假日'} · 原本 ${shortDate(item.movedFrom)}`) : null);
  }));
}

// ---------- 頁面：桌寵 ----------

function currentPet() {
  return state.pets.find((x) => x.id === state.config?.pet) || state.pets[0];
}

function currentPetImage() {
  const p = currentPet();
  return p ? p.image : '';
}

/** 側欄上方：現在這隻桌寵的大頭照與名字。 */
function updateBrand() {
  const p = currentPet();
  document.getElementById('brand-pet').src = p ? p.image : '';
  document.getElementById('brand-name').textContent = p ? p.name : '柚伴';
}

function renderPets(page) {
  page.append(h('div', { class: 'page-head' },
    h('h1', {}, '桌寵'),
    segmented(state.sizes.map((s, i) => [s, SIZE_LABELS[i]]), state.config.sizeDip, async (s) => { state.config.sizeDip = s; await saveConfig(); render(); })));
  page.append(h('div', { class: 'pet-grid' },
    state.pets.map((p) => {
      const current = p.id === state.config.pet;
      return h('button', { class: 'pet-card', 'aria-pressed': String(current), onclick: async () => {
        if (current) return;
        state.config.pet = p.id;
        await saveConfig();
        updateBrand();
        toast(`已換成 ${p.name}`);
        render();
      } },
      current ? h('span', { class: 'badge' }, icon('check'), '使用中') : null,
      h('span', { class: 'stage' }, h('img', { src: p.image, alt: '' })),
      h('span', { class: 'pet-name' }, p.name),
      p.days ? h('span', { class: 'days' }, icon('heart'), `陪你 ${p.days} 天`) : null);
    }).map((card, i) => h('div', { class: 'pet-cell' }, card,
      state.pets[i].editable ? h('button', { class: 'btn btn-secondary btn-small pet-edit', onclick: () => openPetEdit(state.pets[i].id) }, icon('play'), '編輯動作') : null)),
    h('div', { class: 'pet-cell' },
      h('button', { class: 'pet-card add', onclick: openNewPet }, h('span', { class: 'add-plus' }, icon('plus')), h('span', { class: 'pet-name' }, '新增桌寵')))));
  page.append(h('div', { class: 'pets-more' },
    h('button', { class: 'btn btn-ghost btn-small', onclick: () => invoke('open_pets_dir') }, icon('folder-open'), '打開桌寵資料夾')));
  page.append(renderBehavior());
}

/** 讀使用者選的（或拖進來的）圖，交給後端整理。 */
function readImage(file) {
  return new Promise((resolve, reject) => {
    if (!file || !file.type.startsWith('image/')) { reject(new Error('請選一張圖片')); return; }
    const reader = new FileReader();
    reader.onload = () => resolve(reader.result);
    reader.onerror = () => reject(reader.error);
    reader.readAsDataURL(file);
  });
}

/** 一塊可以點、也可以把圖拖進來的區域；`multiple` = 一次可以給好幾張，`onFiles` 收到的是陣列。 */
function imageDrop(cls, content, onFiles, multiple = false) {
  const input = h('input', { type: 'file', accept: 'image/*', hidden: true, multiple, onchange: (e) => { if (e.target.files.length) onFiles([...e.target.files]); } });
  const hasFiles = (e) => [...(e.dataTransfer?.types || [])].includes('Files');
  return h('label', {
    class: cls,
    ondragover: (e) => { if (!hasFiles(e)) return; e.preventDefault(); e.currentTarget.classList.add('dragging'); },
    ondragleave: (e) => e.currentTarget.classList.remove('dragging'),
    ondrop: (e) => {
      if (!hasFiles(e)) return;
      e.preventDefault();
      e.currentTarget.classList.remove('dragging');
      const files = [...e.dataTransfer.files];
      if (files.length) onFiles(multiple ? files : files.slice(0, 1));
    },
  }, input, ...content);
}

// ---------- 小動畫：照實際速度一直播 ----------

let players = [];
const playerStart = performance.now();

/** 一個會自己播的小圖；只有一張就停在那張。 */
function player(frames, fps, cls) {
  const img = h('img', { class: cls, src: frames[0] || '', alt: '' });
  if (frames.length > 1) players.push({ img, frames, fps });
  return img;
}

function tickPlayers() {
  // 畫面重畫後舊的小圖已經不在頁面上，順手丟掉，清單不會越積越長。
  players = players.filter((p) => p.img.isConnected);
  const t = (performance.now() - playerStart) / 1000;
  for (const p of players) {
    const src = p.frames[Math.floor(t * p.fps) % p.frames.length];
    if (p.img.getAttribute('src') !== src) p.img.src = src;
  }
}

function openDrawer(build) {
  editor = { pet: true };
  document.getElementById('drawer-backdrop').hidden = false;
  const drawer = document.getElementById('drawer');
  drawer.setAttribute('aria-hidden', 'false');
  build(drawer);
  requestAnimationFrame(() => drawer.classList.add('open'));
}

async function reloadPets() {
  state.pets = await invoke('list_pets');
  updateBrand();
}

function openNewPet() {
  const draft = { data: null, preview: null, name: '', busy: false };
  openDrawer((drawer) => {
    const draw = () => {
      const ready = draft.preview && draft.name.trim() && !draft.busy;
      const pick = async ([file]) => {
        draft.busy = true;
        draw();
        try {
          draft.data = await readImage(file);
          draft.preview = await invoke('import_preview', { data: draft.data });
        } catch (err) {
          toast(String(err.message || err));
        }
        draft.busy = false;
        draw();
        drawer.querySelector('#pet-name')?.focus();
      };
      drawer.replaceChildren(
        h('div', { class: 'drawer-head' },
          h('h2', {}, '新增桌寵'),
          h('button', { class: 'btn btn-ghost btn-small', 'aria-label': '關閉', onclick: closeEditor }, icon('x'))),
        h('div', { class: 'drawer-body' },
          draft.preview
            ? h('div', { class: 'new-pet-preview' },
              h('span', { class: 'stage big' }, Object.assign(player(draft.preview, 4), { alt: '整理好的桌寵' })),
              draft.preview.length > 1 ? h('div', { class: 'piece-strip' }, draft.preview.map((src) => h('img', { src, alt: '' }))) : null,
              imageDrop('btn btn-secondary btn-small', [icon('folder-open'), '換一張'], pick))
            : imageDrop(`drop-zone${draft.busy ? ' busy' : ''}`, [
              h('span', { class: 'drop-icon' }, icon('plus')),
              h('span', { class: 'drop-title' }, draft.busy ? '整理中…' : '把圖拖進來，或點這裡選一張'),
            ], pick),
          draft.preview ? field('名字', h('input', { id: 'pet-name', class: 'input', placeholder: '例：小白', maxlength: NAME_MAX, value: draft.name,
            oninput: (e) => { draft.name = e.target.value; drawer.querySelector('#create-pet').disabled = !(draft.preview && draft.name.trim()); } }), 'pet-name') : null),
        h('div', { class: 'drawer-foot' },
          h('span', { class: 'spacer' }),
          h('button', { class: 'btn btn-ghost', onclick: closeEditor }, '取消'),
          h('button', { id: 'create-pet', class: 'btn btn-primary', disabled: !ready, onclick: async () => {
            try {
              const id = await invoke('create_pet', { name: draft.name.trim(), data: draft.data });
              state.config.pet = id;
              await saveConfig();
              await reloadPets();
              closeEditor();
              toast(`已換成 ${draft.name.trim()}`);
              await openPetEdit(id);
            } catch (err) {
              toast(String(err));
            }
          } }, icon('check'), '完成')));
    };
    draw();
  });
}

// ---------- 頁面：動作編輯 ----------

async function openPetEdit(id) {
  const states = Object.keys(ACTIONS);
  const [actions, examples] = await Promise.all([
    invoke('pet_actions', { id }),
    Promise.all(states.map((s) => invoke('example_frames', { state: s }))),
  ]);
  state.edit = { id, actions, examples: Object.fromEntries(states.map((s, i) => [s, examples[i]])), prompt: null, busy: null };
  state.page = 'pet-edit';
  render();
  document.getElementById('page').parentElement.scrollTop = 0;
}

/** 改完圖：更新畫面；正在用這隻的話叫桌寵重新載入，馬上在桌面上看到。 */
async function applyActions(promise, s) {
  const e = state.edit;
  try {
    e.actions = await promise;
    if (state.config.pet === e.id && state.coreRunning) await invoke('send_core', { command: { cmd: 'switchPet', id: e.id } });
    if (s === 'idle') await reloadPets();
  } catch (err) {
    toast(String(err.message || err));
  }
  e.busy = null;
  render();
}

function renderPetEdit(page) {
  const e = state.edit;
  const pet = state.pets.find((p) => p.id === e.id);
  if (!pet) { state.page = 'pets'; renderPets(page); return; }
  page.append(h('div', { class: 'page-head' },
    h('div', { class: 'row' },
      h('button', { class: 'btn btn-ghost btn-small', onclick: () => go('pets') }, icon('chevron-left'), '桌寵'),
      h('h1', {}, pet.name)),
    h('button', { class: 'btn btn-danger btn-small', onclick: async () => {
      if (!await confirmDialog({ title: `刪除「${pet.name}」？`, detail: '牠的動作圖也會一起刪掉，刪了就救不回來', confirm: '刪除' })) return;
      try {
        await invoke('delete_pet', { id: pet.id });
      } catch (err) {
        toast(`刪不掉：${err}`);
        return;
      }
      if (state.config.pet === pet.id) state.config.pet = state.pets.find((p) => p.id !== pet.id)?.id || state.config.pet;
      if (state.config.petNames) delete state.config.petNames[pet.id];
      await saveConfig();
      await reloadPets();
      toast(`已刪除 ${pet.name}`);
      go('pets');
    } }, icon('trash-2'), '刪除這隻')));
  for (const group of ACTION_GROUPS) {
    page.append(h('section', { class: 'section' },
      h('h2', {}, group.title),
      h('div', { class: 'card action-list' }, group.items.map(actionRow))));
  }
}

function actionRow(s) {
  const e = state.edit;
  const a = ACTIONS[s];
  const fps = e.actions.fps[s];
  const base = e.actions.defaultFps[s];
  const groups = e.actions.groups.filter((g) => g.state === s);
  if (!groups.length) groups.push({ state: s, variant: DEFAULT_VARIANT, frames: [] });
  const example = e.examples[s] || [];
  const hasAnimation = groups.some((g) => g.frames.length > 1);
  const speed = Math.abs(fps - base) < 0.01 ? 'normal' : fps < base ? 'slow' : 'fast';
  const nextVariant = String(groups.length + 1);
  return h('div', { class: 'action', id: `action-${s}` },
    h('div', { class: 'action-head' },
      h('div', { class: 'action-name' }, h('div', { class: 'action-title' }, a.label), h('div', { class: 'action-hint' }, a.hint)),
      hasAnimation ? h('div', { class: 'action-speed' }, segmented(SPEEDS.map(([k, label]) => [k, label]), speed, (k) => {
        const mul = SPEEDS.find(([key]) => key === k)[2];
        applyActions(invoke('set_action_speed', { id: e.id, state: s, fps: k === 'normal' ? null : Math.round(base * mul * 10) / 10 }));
      })) : null,
      h('button', { class: `btn btn-ghost btn-small${e.prompt === s ? ' active' : ''}`, onclick: () => { e.prompt = e.prompt === s ? null : s; render(); } }, icon('copy'), '給 AI 的提示詞')),
    e.prompt === s ? promptPanel(s, example) : null,
    groups.map((g, gi) => actionGroup(s, g, gi, groups.length, fps, example)),
    s === 'idle' ? imageDrop('btn btn-ghost btn-small add-group', [icon('plus'), '再加一組待機'], (files) => addFrames(s, nextVariant, files), true) : null);
}

function actionGroup(s, g, gi, count, fps, example) {
  const e = state.edit;
  const busy = e.busy === `${s}:${g.variant}`;
  const thumbs = g.frames.map((src, i) => h('div', {
    class: 'thumb',
    draggable: 'true',
    title: `第 ${i + 1} 張`,
    ondragstart: (ev) => ev.dataTransfer.setData('text/x-frame', `${g.variant}:${i}`),
    ondragover: (ev) => { if ([...ev.dataTransfer.types].includes('text/x-frame')) { ev.preventDefault(); ev.currentTarget.classList.add('drop-before'); } },
    ondragleave: (ev) => ev.currentTarget.classList.remove('drop-before'),
    ondrop: (ev) => {
      const [variant, from] = ev.dataTransfer.getData('text/x-frame').split(':');
      if (variant !== g.variant) return;
      ev.preventDefault();
      if (Number(from) !== i) applyActions(invoke('move_frame', { id: e.id, state: s, variant: g.variant, from: Number(from), to: i }), s);
    },
  },
  h('span', { class: 'thumb-no' }, i + 1),
  h('img', { src, alt: '' }),
  h('button', { class: 'thumb-remove', 'aria-label': `刪掉第 ${i + 1} 張`, onclick: () => applyActions(invoke('remove_frame', { id: e.id, state: s, variant: g.variant, index: i }), s) }, icon('x'))));
  return h('div', { class: 'action-group' },
    h('div', { class: 'compare' },
      // 範例只放在第一組旁邊；其他組留同寬的空位，你的圖才會對齊。
      gi === 0
        ? h('div', { class: 'compare-cell' }, example.length ? player(example, fps, 'compare-img') : h('span', { class: 'compare-empty' }), h('span', { class: 'compare-label' }, '範例'))
        : h('div', { class: 'compare-cell spacer' }),
      h('div', { class: `compare-cell mine${g.frames.length ? '' : ' empty'}` },
        g.frames.length ? player(g.frames, fps, 'compare-img') : h('span', { class: 'compare-empty' }, icon('plus')),
        h('span', { class: 'compare-label' }, count > 1 ? `第 ${gi + 1} 組` : '你的'))),
    h('div', { class: 'strip' },
      thumbs,
      imageDrop(`thumb add${busy ? ' busy' : ''}`, [icon('plus'), h('span', {}, busy ? '整理中…' : '加圖')], (files) => addFrames(s, g.variant, files), true)));
}

async function addFrames(s, variant, files) {
  const e = state.edit;
  e.busy = `${s}:${variant}`;
  render();
  try {
    const images = await Promise.all(files.map(readImage));
    await applyActions(invoke('add_frames', { id: e.id, state: s, variant, images }), s);
  } catch (err) {
    e.busy = null;
    toast(String(err.message || err));
    render();
  }
}

function promptPanel(s, example) {
  const text = actionPrompt(s);
  return h('div', { class: 'prompt-panel' },
    h('pre', { class: 'prompt-text' }, text),
    h('div', { class: 'row' },
      h('button', { class: 'btn btn-primary btn-small', onclick: async () => { await navigator.clipboard.writeText(text); toast('已複製提示詞'); } }, icon('copy'), '複製提示詞'),
      example.length ? h('button', { class: 'btn btn-secondary btn-small', onclick: () => invoke('save_example', { state: s, label: ACTIONS[s].label }).then(() => toast('範例圖已存到「下載」')).catch((err) => toast(String(err))) }, icon('download'), '下載範例圖') : null));
}

/** 活動範圍選了「只在範圍內」或「避開這些地方」時，底下列出畫好的框。框由核心在螢幕上畫，畫好寫進設定檔。 */
/** 叫核心在桌面上框出一個範圍；不帶範圍 = 收掉。核心沒在跑就算了。 */
function previewArea(kind, area) {
  invoke('send_core', { command: area ? { cmd: 'previewArea', kind, area } : { cmd: 'previewArea' } }).catch(() => {});
}

function areaRows() {
  const draw = (kind) => invoke('send_core', { command: { cmd: 'drawArea', kind } });
  const size = (a) => h('span', { class: 'tag' }, `${a.right - a.left} × ${a.bottom - a.top}`);
  const drawButton = (kind, glyph, label) => h('button', { class: 'btn btn-secondary btn-small', disabled: !state.coreRunning, onclick: () => draw(kind) }, icon(glyph), label);
  // 滑鼠移到某個範圍那一列：桌面上框出那塊範圍，移開就收掉。
  // 標題前的小方塊長得跟桌面上的框一樣，滑過時整列染同一個顏色（`.area-row`）。
  const swatch = () => h('span', { class: 'area-swatch' });
  const preview = (row, kind, area) => {
    row.classList.add('area-row', `area-${kind}`);
    row.querySelector('.setting-title').prepend(swatch());
    row.addEventListener('mouseenter', () => previewArea(kind, area));
    row.addEventListener('mouseleave', () => previewArea());
    return row;
  };
  if (state.config.roam === 'inside') {
    const a = state.config.allowedArea;
    const row = settingRow('範圍', h('div', { class: 'row' }, a ? size(a) : null, a ? drawButton('allowed', 'rotate-ccw', '重畫') : drawButton('allowed', 'plus', '畫範圍')));
    return [a ? preview(row, 'allowed', a) : row];
  }
  if (state.config.roam === 'avoid') {
    const remove = async (i) => { previewArea(); state.config.forbiddenAreas.splice(i, 1); await saveConfig(); render(); };
    return [
      ...state.config.forbiddenAreas.map((a, i) => preview(settingRow(`禁止範圍 ${i + 1}`, h('div', { class: 'row' }, size(a),
        h('button', { class: 'btn btn-danger btn-small', 'aria-label': `刪除禁止範圍 ${i + 1}`, onclick: () => remove(i) }, icon('trash-2'), '刪除'))), 'forbidden', a)),
      settingRow('', drawButton('forbidden', 'plus', '新增禁止範圍')),
    ];
  }
  return [];
}

/** 現在這隻的名字；清空就回到桌寵包自己的名字（顯示成 placeholder）。 */
function petNameInput() {
  const p = currentPet();
  return h('input', { class: 'input input-name', 'aria-label': '名字', maxlength: NAME_MAX, placeholder: p.species, value: state.config.petNames?.[p.id] || '',
    onchange: async (e) => {
      const names = { ...state.config.petNames };
      const name = e.target.value.trim();
      if (name) names[p.id] = name;
      else delete names[p.id];
      state.config.petNames = names;
      await saveConfig();
      await reloadPets();
      render();
    } });
}

/** 牠怎麼叫你（打招呼、歡迎回來時用）；空的就不叫名字。 */
function userNameInput() {
  return h('input', { class: 'input input-name', 'aria-label': '怎麼叫你', maxlength: NAME_MAX, placeholder: '例：小明', value: state.config.userName || '',
    onchange: async (e) => {
      state.config.userName = e.target.value.trim();
      e.target.value = state.config.userName;
      await saveConfig();
    } });
}

function renderBehavior() {
  const set = async (key, value) => { state.config[key] = value; await saveConfig(); render(); };
  return h('section', { class: 'section' },
    h('h2', {}, '行為'),
    h('div', { class: 'card' },
      settingRow('名字', petNameInput()),
      settingRow('怎麼叫你', userNameInput()),
      settingRow('活潑程度', segmented(ACTIVITY_CHOICES, state.config.activity, (v) => set('activity', v))),
      settingRow('活動範圍', segmented(ROAM_CHOICES, state.config.roam, (v) => set('roam', v))),
      ...areaRows(),
      settingRow('講話', segmented(TALK_CHOICES, state.config.talk, (v) => set('talk', v))),
      settingRow('沒碰電腦多久就睡覺', segmented(SLEEP_CHOICES, state.config.sleepAfterSecs, (v) => set('sleepAfterSecs', v))),
      settingRow('站到視窗上', configSwitch('standOnWindows', '站到視窗上')),
      settingRow('全螢幕時躲起來', configSwitch('hideInFullscreen', '全螢幕時躲起來'))));
}

// ---------- 頁面：視窗歸位 ----------

async function refreshWindows() {
  state.windows = await invoke('window_layout');
}

function renderWindows(page) {
  if (featureOff(page, 'windowRestore')) return;
  const act = async (cmd, done) => {
    await invoke('send_core', { command: { cmd } });
    toast(done);
    setTimeout(async () => { await refreshWindows(); render(); }, LAYOUT_REFRESH_MS);
  };
  page.append(h('div', { class: 'page-head' },
    featureTitle('windowRestore'),
    h('div', { class: 'row' },
      h('button', { class: 'btn btn-secondary', disabled: !state.coreRunning, onclick: () => act('restoreLayout', '已恢復') }, icon('rotate-ccw'), '恢復位置'),
      h('button', { class: 'btn btn-primary', disabled: !state.coreRunning, onclick: () => act('saveLayout', '已記住目前位置') }, icon('save'), '記住目前位置'))));
  if (!state.windows.length) {
    page.append(h('div', { class: 'card empty' },
      h('img', { src: currentPetImage(), alt: '' }),
      h('div', { class: 'empty-title' }, '還沒有記住任何視窗')));
  } else {
    page.append(h('div', { class: 'card list' }, state.windows.map((w) => h('div', { class: 'list-row clickable', onclick: () => openWindowEdit(w) },
      h('div', { class: 'row-main' },
        h('div', { class: 'row-title' }, w.title),
        h('div', { class: 'row-meta' }, w.exe)),
      h('span', { class: 'tag' }, `${w.left}, ${w.top}　${w.width} × ${w.height}`)))));
  }
  page.append(h('div', { class: 'section' }, h('div', { class: 'card' },
    settingRow('記住新視窗時說一聲', configSwitch('announceNewWindows', '記住新視窗時說一聲')))));
}

/** 直接打數字改一個視窗記下的位置，套用時馬上把視窗搬過去。 */
function openWindowEdit(w) {
  const draft = { left: w.left, top: w.top, width: w.width, height: w.height };
  const valid = () => Object.values(draft).every(Number.isInteger) && draft.width > 0 && draft.height > 0;
  openDrawer((drawer) => {
    const num = (key, label, min) => field(label, h('input', { id: `win-${key}`, class: 'input', type: 'number', step: 1, min, value: draft[key],
      oninput: (e) => { draft[key] = e.target.valueAsNumber; drawer.querySelector('#apply-window').disabled = !valid() || !state.coreRunning; } }), `win-${key}`);
    drawer.replaceChildren(
      h('div', { class: 'drawer-head' },
        h('h2', {}, '編輯位置'),
        h('button', { class: 'btn btn-ghost btn-small', 'aria-label': '關閉', onclick: closeEditor }, icon('x'))),
      h('div', { class: 'drawer-body' },
        h('div', { class: 'window-edit-name' }, h('div', { class: 'row-title' }, w.title), h('div', { class: 'row-meta' }, w.exe)),
        h('div', { class: 'field-pair' }, num('left', 'X'), num('top', 'Y')),
        h('div', { class: 'field-pair' }, num('width', '寬', 1), num('height', '高', 1))),
      h('div', { class: 'drawer-foot' },
        h('span', { class: 'spacer' }),
        h('button', { class: 'btn btn-ghost', onclick: closeEditor }, '取消'),
        h('button', { id: 'apply-window', class: 'btn btn-primary', disabled: !state.coreRunning, onclick: async () => {
          await invoke('send_core', { command: { cmd: 'editWindow', id: w.id, ...draft } });
          closeEditor();
          toast('已套用');
          setTimeout(async () => { await refreshWindows(); if (state.page === 'windows') render(); }, LAYOUT_REFRESH_MS);
        } }, icon('check'), '套用')));
    setTimeout(() => drawer.querySelector('#win-left')?.focus(), 50);
  });
}

// ---------- 頁面：專注 ----------

function renderFocus(page) {
  if (featureOff(page, 'focus')) return;
  const c = state.config;
  const set = async (key, value) => { c[key] = value; await saveConfig(); render(); };
  page.append(h('div', { class: 'page-head' },
    featureTitle('focus'),
    focusButton()));
  page.append(h('div', { class: 'card' },
    settingRow('專注', segmented(FOCUS_CHOICES, c.focusMinutes, (v) => set('focusMinutes', v))),
    settingRow('短休息', segmented(SHORT_BREAK_CHOICES, c.shortBreakMinutes, (v) => set('shortBreakMinutes', v))),
    settingRow(`長休息（每 ${c.focusRoundsBeforeLong} 輪一次）`, segmented(LONG_BREAK_CHOICES, c.longBreakMinutes, (v) => set('longBreakMinutes', v))),
    settingRow('休息完自動開始下一輪', configSwitch('focusAutoContinue', '休息完自動開始下一輪')),
    settingRow('卡片', segmented(STYLE_CHOICES, c.focusStyle, (v) => set('focusStyle', v))),
    settingRow('倒數', segmented(COUNTDOWN_CHOICES, c.focusCountdown, (v) => set('focusCountdown', v)))));
}

/** 沒在專注是「開始專注」，專注／休息中跟右鍵選單一樣是「停止專注（還剩幾分）」。 */
function focusButton() {
  const send = async (cmd) => { await invoke('send_core', { command: { cmd } }); await refreshFocus(); };
  return state.focusStopLabel
    ? h('button', { class: 'btn btn-secondary', disabled: !state.coreRunning, onclick: () => send('focusStop') }, icon('x'), state.focusStopLabel)
    : h('button', { class: 'btn btn-primary', disabled: !state.coreRunning, onclick: () => send('focusStart') }, icon('tomato'), '開始專注');
}

/** 核心處理完指令才回來（`SendMessage`），送完馬上讀就是新的狀態。 */
async function refreshFocus() {
  const label = await invoke('focus_stop_label');
  if (label === state.focusStopLabel) return;
  state.focusStopLabel = label;
  if (state.page === 'focus') render();
}

// ---------- 頁面：一般 ----------

function renderGeneral(page) {
  page.append(h('div', { class: 'page-head' }, h('h1', {}, '一般')));
  const autostart = h('label', { class: 'switch' },
    h('input', { type: 'checkbox', checked: !!state.autostart, disabled: state.autostart === null, 'aria-label': '開機自動啟動', onchange: async (e) => {
      const on = e.target.checked;
      state.autostartError = '';
      try {
        await invoke('set_autostart', { on });
        state.autostart = on;
        toast(on ? '已設定開機自動啟動' : '已取消開機自動啟動');
      } catch (err) {
        state.autostartError = String(err);
      }
      state.autostart = await invoke('autostart_status').catch(() => state.autostart);
      render();
    } }),
    h('span'));
  page.append(h('div', { class: 'card' },
    h('div', { class: 'setting' },
      h('div', { class: 'setting-main' },
        h('div', { class: 'setting-title' }, '開機自動啟動'),
        state.autostartError ? h('div', { class: 'setting-error' }, state.autostartError) : null),
      autostart),
    settingRow('自動更新', configSwitch('autoUpdate', '自動更新')),
    updateRow(),
    settingRow('外觀', segmented(COLOR_SCHEMES, state.config.colorScheme, async (v) => { state.config.colorScheme = v; await saveConfig(); render(); }))));
}

function updateRow() {
  const u = state.update;
  const busy = UPDATE_BUSY.includes(u?.state);
  const text = {
    checking: '檢查中…',
    downloading: `下載新版 ${u?.version} 中…`,
    upToDate: '已經是最新版',
    ready: `新版 ${u?.version} 準備好了`,
    failed: '連不上更新伺服器，請稍後再試',
    unavailable: '開發中的版本不會更新',
  }[u?.state];
  const button = u?.state === 'ready'
    ? h('button', { class: 'btn btn-primary btn-small', disabled: !state.coreRunning, onclick: () => invoke('apply_update') }, '現在更新')
    : h('button', { class: 'btn btn-secondary btn-small', disabled: busy || !state.coreRunning, onclick: checkUpdate }, '檢查更新');
  return h('div', { class: 'setting' },
    h('div', { class: 'setting-main' },
      h('div', { class: 'setting-title' }, `目前版本 ${state.version}`),
      text ? h('div', { class: u.state === 'failed' ? 'setting-error' : 'setting-desc', role: 'status' }, text) : null),
    button);
}

async function checkUpdate() {
  await invoke('send_core', { command: { cmd: 'checkUpdate' } });
  await pollUpdate();
}

/** 核心查完會把結果寫進檔案；查的時候每半秒讀一次。 */
async function pollUpdate() {
  clearTimeout(pollUpdate.timer);
  state.update = await invoke('update_status');
  if (state.page === 'general') render();
  if (UPDATE_BUSY.includes(state.update?.state)) pollUpdate.timer = setTimeout(pollUpdate, UPDATE_POLL_MS);
}

// ---------- 骨架 ----------

const PAGES = { reminders: renderReminders, pets: renderPets, windows: renderWindows, focus: renderFocus, general: renderGeneral, 'pet-edit': renderPetEdit };
/** 不在側欄上的頁面算在哪一個分類底下。 */
const NAV_PARENT = { 'pet-edit': 'pets' };

function render() {
  document.documentElement.dataset.theme = state.config.colorScheme;
  const nav = NAV_PARENT[state.page] || state.page;
  for (const b of document.querySelectorAll('.nav-item')) {
    if (b.dataset.page === nav) b.setAttribute('aria-current', 'page'); else b.removeAttribute('aria-current');
  }
  for (const [key, , pageId] of FEATURES) {
    document.querySelector(`.nav-item[data-page="${pageId}"]`).classList.toggle('off', !state.config.features[key]);
  }
  const page = document.getElementById('page');
  page.replaceChildren();
  PAGES[state.page](page);
}

async function go(name) {
  state.page = PAGES[name] && !NAV_PARENT[name] ? name : DEFAULT_PAGE;
  if (state.page === 'windows') await refreshWindows();
  if (state.page === 'general' && state.autostart === null) state.autostart = await invoke('autostart_status').catch(() => false);
  if (state.page === 'general' && state.update === null) {
    // 「已經是最新版」「連不上」是之前哪一次查的結果看不出來，不顯示；還在查、已經下載好的才接著顯示。
    const u = await invoke('update_status');
    if (u && (u.state === 'ready' || UPDATE_BUSY.includes(u.state))) {
      state.update = u;
      if (UPDATE_BUSY.includes(u.state)) pollUpdate();
    }
  }
  render();
}

async function pollCore() {
  const running = await invoke('core_running');
  document.getElementById('core-banner').hidden = running;
  document.getElementById('brand-status').hidden = !running;
  if (running !== state.coreRunning) {
    state.coreRunning = running;
    render();
  }
  await refreshFocus();
}

async function main() {
  applyTokens(await invoke('tokens'));
  const [config, constants, pets] = await Promise.all([invoke('load_config'), invoke('constants'), invoke('list_pets')]);
  state.config = config;
  state.sizes = constants.sizes;
  state.lunarMonths = constants.lunarMonths;
  state.lunarDays = constants.lunarDays;
  state.version = constants.version;
  document.getElementById('app-version').textContent = constants.version;
  state.pets = pets;
  await loadReminders();
  updateBrand();
  for (const b of document.querySelectorAll('.nav-item')) b.addEventListener('click', () => go(b.dataset.page));
  document.getElementById('drawer-backdrop').addEventListener('click', closeEditor);
  document.addEventListener('keydown', (e) => { if (e.key === 'Escape' && editor) closeEditor(); });
  document.getElementById('start-core').addEventListener('click', async () => { await invoke('start_core'); setTimeout(pollCore, 800); });
  // 切到別的視窗時，滑鼠移開範圍那一列的通知收不到：框一起收掉。
  window.addEventListener('blur', () => previewArea());
  // 核心也會寫設定檔（畫好範圍、從托盤換桌寵）：回到這個視窗時重新讀，畫面才不會停在舊的。
  window.addEventListener('focus', async () => {
    const config = await invoke('load_config');
    if (JSON.stringify(config) !== JSON.stringify(state.config)) {
      state.config = config;
      render();
    }
  });
  await pollCore();
  await go((await invoke('initial_page')) || DEFAULT_PAGE);
  setInterval(pollCore, CORE_POLL_MS);
  setInterval(tickPlayers, PLAYER_TICK_MS);
}

main();
