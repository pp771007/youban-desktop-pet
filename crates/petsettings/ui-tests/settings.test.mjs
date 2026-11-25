// 設定畫面的驗收測試：用無頭瀏覽器開 ui/index.html，Tauri 的 invoke 換成記憶體裡的假後端。
// 跑法：node crates/petsettings/ui-tests/settings.test.mjs [截圖資料夾]
// （playwright 裝在這個資料夾或任何上層的 node_modules 都可以，node 會往上層找。）

import { chromium } from 'playwright';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { createServer } from 'node:http';
import { readFile } from 'node:fs/promises';
import path from 'node:path';

const here = path.dirname(fileURLToPath(import.meta.url));
const uiDir = path.join(here, '..', 'ui');
const MIME = { '.html': 'text/html; charset=utf-8', '.js': 'text/javascript', '.css': 'text/css', '.svg': 'image/svg+xml' };

// 用 http 而不是 file://：CSS 遮罩圖（圖示）要同源才會載入，file:// 沒有來源會被擋掉。
const server = createServer(async (req, res) => {
  try {
    const file = path.join(uiDir, decodeURIComponent(new URL(req.url, 'http://x').pathname));
    if (!file.startsWith(uiDir)) throw new Error('outside ui');
    const body = await readFile(file.endsWith(path.sep) ? path.join(file, 'index.html') : file);
    res.writeHead(200, { 'content-type': MIME[path.extname(file)] || 'application/octet-stream' });
    res.end(body);
  } catch {
    res.writeHead(404);
    res.end();
  }
});
await new Promise((r) => server.listen(0, '127.0.0.1', r));
const pageUrl = `http://127.0.0.1:${server.address().port}/index.html`;
const shotDir = process.argv[2];
const tokens = (await import(pathToFileURL(path.join(here, '..', '..', '..', 'design', 'tokens.json')).href, { with: { type: 'json' } })).default;

// 假後端：跟 src/main.rs 的指令同名同參數。
function mockBackend(tokens) {
  // 整個函式會被送進頁面裡跑，用到的東西都要寫在裡面（同 petkit::anniversary::LUNAR_MONTHS / LUNAR_DAYS）。
  const LUNAR_MONTHS = ['正月', '二月', '三月', '四月', '五月', '六月', '七月', '八月', '九月', '十月', '十一月', '十二月'];
  const LUNAR_DAYS = ['初一', '初二', '初三', '初四', '初五', '初六', '初七', '初八', '初九', '初十', '十一', '十二', '十三', '十四', '十五', '十六', '十七', '十八', '十九', '二十', '廿一', '廿二', '廿三', '廿四', '廿五', '廿六', '廿七', '廿八', '廿九', '三十'];
  const db = {
    config: { pet: 'capybara', sizeDip: 128, sleepAfterSecs: 300, activity: 'normal', roam: 'screen', talk: 'sometimes', standOnWindows: true, hideInFullscreen: true, reminderSound: true, remindInFullscreen: false, autoUpdate: true, announceNewWindows: true, restReminder: true, restEveryMinutes: 50, restStyle: 'notice', focusMinutes: 25, shortBreakMinutes: 5, longBreakMinutes: 15, focusRoundsBeforeLong: 4, focusAutoContinue: true, focusStyle: 'notice', focusCountdown: 'always', allowedArea: null, forbiddenAreas: [{ left: 100, top: 900, right: 700, bottom: 1040 }], features: { reminders: true, windowRestore: true, focus: true, relay: true } },
    file: {
      reminders: [
        { id: 'a', title: '喝水', enabled: true, time: '14:00', repeat: { kind: 'daily' }, holiday: 'normal', startDate: '2026-01-01' },
        { id: 'b', title: '繳卡費', enabled: false, time: '10:30', repeat: { kind: 'monthly', rule: { kind: 'day', day: 25 } }, holiday: 'prevWorkday', startDate: '2026-01-01' },
      ],
      anniversaries: [
        { id: 'mom', title: '媽媽生日', enabled: true, date: { calendar: 'lunar', month: 3, day: 5, leap: false }, year: 1966, alerts: [{ daysBefore: 3, time: '09:00' }, { daysBefore: 0, time: '09:00' }] },
      ],
      dayOverrides: {},
    },
    relayLog: {
      seenUntil: null,
      entries: [
        { key: '#1', title: '備份跑完了', detail: null, from: null, at: '2026-11-24T08:30:00', style: 'notice', done: false },
        { key: '#2', title: 'PR 等你審', detail: '有 2 則新留言', from: 'Claude', at: '2026-11-24T09:00:00', style: 'confirm', done: false },
        { key: '#3', title: '建置完成', detail: null, from: 'Claude', at: '2026-11-24T09:05:00', style: 'notice', done: false },
      ],
    },
    calls: [],
    running: true,
    // 上一次（很久以前）查的結果還留在檔案裡。
    updateSteps: [{ state: 'upToDate' }],
  };
  const png = 'data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+ip1sAAAAASUVORK5CYII=';
  db.pets = [{ id: 'capybara', name: '水豚', image: png, editable: false, days: 12 }, { id: 'capybara-side', name: '水豚（側面）', image: png, editable: false, days: 0 }];
  const allStates = ['idle', 'walk', 'drag', 'fall', 'sleep', 'notify', 'blink', 'happy', 'sit', 'land', 'yawn', 'wake', 'talk', 'hop', 'look', 'doze', 'climb', 'dizzy'];
  db.actions = { groups: [{ state: 'idle', variant: 'default', frames: [png] }], fps: Object.fromEntries(allStates.map((s) => [s, 8])), defaultFps: Object.fromEntries(allStates.map((s) => [s, 8])) };
  const handlers = {
    initial_page: () => null,
    tokens: () => tokens,
    constants: () => ({ sizes: [96, 128, 176], version: '0.1.0', lunarMonths: LUNAR_MONTHS, lunarDays: LUNAR_DAYS }),
    // 真的換算在 Rust（petkit::anniversary 有測）；這裡只回固定的「下一次在 2027/4/11、還有 12 天」。
    anniversary_info: ({ anniversary: a }) => ({
      label: a.date.calendar === 'lunar' ? `農曆${a.date.leap ? '閏' : ''}${LUNAR_MONTHS[a.date.month - 1]}${LUNAR_DAYS[a.date.day - 1]}` : `國曆 ${a.date.month}/${a.date.day}`,
      upcoming: [2027, 2028, 2029].map((y, i) => ({ date: `${y}-04-${11 + i}`, nth: a.year ? y - a.year : null })),
      daysLeft: 12,
    }),
    load_config: () => structuredClone(db.config),
    save_config: ({ config }) => { db.config = structuredClone(config); },
    load_reminders: () => structuredClone(db.file),
    save_reminders: ({ file }) => { db.file = structuredClone(file); },
    official_days: () => [
      { date: '2026-12-25', kind: 'holiday', note: '行憲紀念日' },
      { date: '2027-01-01', kind: 'holiday', note: '開國紀念日' },
      { date: '2027-02-20', kind: 'workday', note: '補行上班' },
      { date: '2027-10-25', kind: 'holiday', note: '臺灣光復暨金門古寧頭大捷紀念日' },
    ],
    preview: ({ reminder }) => {
      if (reminder.holiday === 'skip') {
        return [{ kind: 'skipped', date: '2027-01-01', note: '開國紀念日' }, { kind: 'fire', at: '2027-01-04T09:00:00', movedFrom: null, note: null }];
      }
      return [{ kind: 'fire', at: '2027-03-02T09:00:00', movedFrom: null, note: null }, { kind: 'fire', at: '2027-03-03T09:00:00', movedFrom: '2027-02-28', note: null }];
    },
    list_pets: () => db.pets.map((p) => ({ ...structuredClone(p), species: p.name, name: db.config.petNames?.[p.id]?.trim() || p.name })),
    import_preview: ({ data }) => { if (!data.startsWith('data:image/')) throw '讀不懂這張圖'; return [png, png, png, png]; },
    create_pet: ({ name }) => { db.pets.push({ id: 'my-1', name, image: png, editable: true, days: 0 }); return 'my-1'; },
    pet_actions: () => structuredClone(db.actions),
    add_frames: ({ state, variant, images }) => {
      db.calls.push({ cmd: 'add_frames', state, variant, count: images.length });
      let g = db.actions.groups.find((x) => x.state === state && x.variant === variant);
      if (!g) db.actions.groups.push(g = { state, variant, frames: [] });
      g.frames.push(...images.map(() => png));
      return structuredClone(db.actions);
    },
    remove_frame: ({ state, variant, index }) => { db.actions.groups.find((x) => x.state === state && x.variant === variant).frames.splice(index, 1); return structuredClone(db.actions); },
    move_frame: ({ state, from, to }) => { db.calls.push({ cmd: 'move_frame', state, from, to }); return structuredClone(db.actions); },
    set_action_speed: ({ state, fps }) => { db.calls.push({ cmd: 'set_action_speed', state, fps }); db.actions.fps[state] = fps ?? db.actions.defaultFps[state]; return structuredClone(db.actions); },
    example_frames: () => [png, png],
    save_example: ({ state }) => { db.calls.push({ cmd: 'save_example', state }); },
    delete_pet: ({ id }) => { db.pets = db.pets.filter((p) => p.id !== id); },
    window_layout: () => [{ id: 4242, title: '記事本', exe: 'notepad.exe', left: 100, top: 50, width: 800, height: 600 }],
    core_running: () => db.running,
    send_core: ({ command }) => {
      db.calls.push(command);
      // 核心查新版（send_core 跟真的一樣做完才回來）：每讀一次進度往前走一步，停在最後一個。
      if (command.cmd === 'checkUpdate') db.updateSteps = [{ state: 'checking' }, { state: 'downloading', version: '0.2.0' }, { state: 'ready', version: '0.2.0' }];
      if (command.cmd === 'focusStart') db.focusStopLabel = '停止專注（還剩 25 分）';
      if (command.cmd === 'focusStop') db.focusStopLabel = null;
      // 傳話紀錄：跟 petkit::relay_log 一樣（清除留著還沒按完成的確認框）。
      const waiting = (e) => e.style === 'confirm' && !e.done;
      if (command.cmd === 'relayLogSeen') db.relayLog.seenUntil = '2026-11-24T23:59:00';
      if (command.cmd === 'relayDone') for (const e of db.relayLog.entries) if (e.key === command.key) e.done = true;
      if (command.cmd === 'relayLogClear') db.relayLog.entries = db.relayLog.entries.filter((e) => waiting(e) || ('from' in command && e.from !== command.from));
    },
    load_relay_log: () => structuredClone(db.relayLog),
    focus_stop_label: () => db.focusStopLabel ?? null,
    update_status: () => (db.updateSteps.length > 1 ? db.updateSteps.shift() : db.updateSteps[0] ?? null),
    apply_update: () => { db.calls.push({ cmd: 'apply_update' }); },
    start_core: () => { db.running = true; },
    open_pets_dir: () => { db.calls.push({ cmd: 'open_pets_dir' }); },
    autostart_status: () => false,
    set_autostart: ({ on }) => { if (on) throw '寫不進登錄（存取被拒）'; },
  };
  window.__mock = db;
  window.__TAURI__ = {
    core: {
      invoke: async (cmd, args) => {
        if (!handlers[cmd]) throw new Error(`no mock for ${cmd}`);
        return handlers[cmd](args || {});
      },
    },
  };
}

let failures = 0;
const check = (cond, label) => {
  console.log(`${cond ? 'PASS' : 'FAIL'} ${label}`);
  if (!cond) failures += 1;
};

const browser = await chromium.launch({ headless: true });
for (const scheme of ['light', 'dark']) {
  const ctx = await browser.newContext({ viewport: { width: 1000, height: 700 }, colorScheme: scheme, locale: 'zh-TW' });
  const page = await ctx.newPage();
  const errors = [];
  page.on('pageerror', (e) => errors.push(e.message));
  await page.addInitScript(mockBackend, tokens);
  await page.goto(pageUrl);
  await page.waitForSelector('.list-row');

  if (scheme === 'light') {
    // 清單：啟用的排前面、暫停的淡掉、顯示重複規則與假日規則
    const rows = await page.locator('.card.list').first().locator('.list-row').allInnerTexts();
    check(rows[0].includes('喝水') && rows[0].includes('每天'), 'enabled reminder listed first with repeat summary');
    check(rows[1].includes('已暫停') && rows[1].includes('遇假日提前'), 'paused reminder shows paused and holiday policy');
    await page.getByRole('checkbox', { name: '提醒音' }).click({ force: true });
    check(await page.evaluate(() => window.__mock.config.reminderSound) === false, 'reminder sound switch writes config');
    await page.getByRole('button', { name: '30 分' }).click();
    await page.locator('.section', { hasText: '休息提醒' }).getByRole('button', { name: '要按完成' }).click();
    check(await page.evaluate(() => { const c = window.__mock.config; return c.restEveryMinutes === 30 && c.restStyle === 'confirm'; }), 'rest reminder interval and card style write config');
    await page.getByRole('checkbox', { name: '休息提醒' }).click({ force: true });
    check(await page.evaluate(() => window.__mock.config.restReminder) === false && !(await page.getByText('連續用多久').isVisible()), 'turning the rest reminder off hides its options');
    await page.getByRole('checkbox', { name: '全螢幕時照樣跳出來' }).click({ force: true });
    check(await page.evaluate(() => window.__mock.config.remindInFullscreen) === true, 'remind-in-fullscreen switch writes config');
    await page.locator('#reminder-style .setting', { hasText: '提示框' }).getByRole('button', { name: '看看樣子' }).click();
    check(await page.evaluate(() => window.__mock.calls.some((c) => c.cmd === 'testReminder' && c.style === 'notice')), 'notice preview shows a notice on the pet');

    // 新增：沒填內容不能存；工作日沒有「遇到假日」；存檔寫出正確的 JSON
    await page.getByRole('button', { name: '新增提醒' }).first().click();
    await page.waitForSelector('.drawer.open');
    // 抽屜打開後焦點會稍晚才落到「提醒內容」：先等它，不然後面打字打到一半焦點被搶走。
    await page.waitForFunction(() => document.activeElement?.id === 'title');
    check(await page.locator('#save-btn').isDisabled(), 'save disabled until title filled');
    await page.locator('#title').fill('站起來走走');
    check(await page.locator('#save-btn').isEnabled(), 'save enabled after title');
    // 時間一個鍵一個鍵打：打到一半欄位不能被換掉（打 30 不能變成 03）
    await page.locator('#time').focus();
    await page.keyboard.type('0930', { delay: 50 });
    const typedTime = await page.locator('#time').inputValue();
    check(typedTime.endsWith(':30'), `typing a time digit by digit keeps every digit (got ${typedTime})`);
    // 定時：開始時間跟結束時間、間隔放在同一塊（上面那格「時間」不出現）
    await page.getByRole('button', { name: '定時' }).click();
    check(await page.getByLabel('從幾點').isVisible() && await page.getByLabel('到幾點').isVisible() && !(await page.getByText('時間', { exact: true }).isVisible()), 'interval shows start and end time together');
    await page.getByRole('button', { name: '工作日' }).click();
    check(!(await page.getByText('遇到假日').isVisible()), 'workdays hides holiday policy');
    await page.getByRole('button', { name: '每週' }).click();
    await page.getByRole('button', { name: '星期一' }).click();
    await page.getByRole('button', { name: '星期三' }).click();
    await page.getByRole('button', { name: '跳過' }).click();
    await page.waitForSelector('.preview-row.skipped');
    check((await page.locator('.preview-row.skipped').innerText()).includes('開國紀念日'), 'preview marks skipped holiday');
    if (shotDir) await page.screenshot({ path: path.join(shotDir, 'settings-edit-light.png') });
    await page.locator('#save-btn').click();
    await page.waitForSelector('.drawer:not(.open)');
    const saved = await page.evaluate(() => window.__mock.file.reminders.find((r) => r.title === '站起來走走'));
    check(saved && saved.repeat.kind === 'weekly' && saved.holiday === 'skip' && saved.repeat.days.length >= 1, 'new reminder saved with weekly + skip');

    // 常用範本：一鍵填好「伸展」、上班時間每 90 分鐘、自己消失
    await page.getByRole('button', { name: '新增提醒' }).first().click();
    await page.waitForFunction(() => document.activeElement?.id === 'title');
    await page.getByRole('button', { name: '伸展', exact: true }).click();
    // 捲到下面選切換鈕：抽屜重畫後要停在原地，不能跳回最上面
    const scrolledTo = await page.evaluate(() => { const b = document.querySelector('.drawer-body'); b.scrollTop = b.scrollHeight; return b.scrollTop; });
    await page.getByRole('button', { name: '要按完成' }).click();
    await page.getByRole('button', { name: '自己消失' }).click();
    const scrollAfter = await page.evaluate(() => document.querySelector('.drawer-body').scrollTop);
    check(scrolledTo > 0 && scrollAfter === scrolledTo, `picking an option keeps the drawer where it was (${scrolledTo} → ${scrollAfter})`);
    if (shotDir) await page.screenshot({ path: path.join(shotDir, 'settings-template-light.png') });
    await page.locator('#save-btn').click();
    await page.waitForSelector('.drawer:not(.open)');
    const stretch = await page.evaluate(() => window.__mock.file.reminders.find((r) => r.title === '起來伸展一下'));
    check(stretch && stretch.repeat.kind === 'interval' && stretch.repeat.everyMinutes === 90 && stretch.time === '09:00' && stretch.repeat.until === '18:00' && stretch.style === 'notice', 'template fills an interval reminder that goes away by itself');
    // 列表上：自己消失的用訊息泡泡圖示、要按完成的用鈴鐺
    const rowIcon = (title) => page.locator('.list-row', { hasText: title }).locator('.row-dot .icon').getAttribute('style');
    check((await rowIcon('起來伸展一下')).includes('message-dots') && (await rowIcon('站起來走走')).includes('bell'), 'list shows a message bubble for go-away reminders and a bell for the rest');
    await page.waitForFunction(() => !document.getElementById('drawer').getBoundingClientRect().width || document.getElementById('drawer').getBoundingClientRect().left >= window.innerWidth);

    // 刪除先跳確認框：取消什麼都不做，確定才刪
    await page.locator('.list-row', { hasText: '站起來走走' }).click();
    await page.getByRole('button', { name: '刪除' }).click();
    await page.getByRole('alertdialog', { name: '刪除「站起來走走」？' }).waitFor();
    if (shotDir) {
      // 確認框本身、紅色按鈕滑鼠移上去、按下去：三種樣子都要看過。
      const dialog = page.getByRole('alertdialog');
      const danger = dialog.getByRole('button', { name: '刪除' });
      await page.screenshot({ path: path.join(shotDir, 'settings-confirm-light.png') });
      await danger.hover();
      await dialog.screenshot({ path: path.join(shotDir, 'confirm-hover.png') });
      await page.mouse.down();
      await dialog.screenshot({ path: path.join(shotDir, 'confirm-pressed.png') });
      // 在框裡別的地方放開：不算按到，框還開著，接著照常用 Esc 取消。
      await dialog.getByRole('heading').hover();
      await page.mouse.up();
    }
    await page.keyboard.press('Escape');
    check(await page.getByRole('alertdialog').count() === 0 && await page.evaluate(() => window.__mock.file.reminders.length) === 4, 'delete asks first and Esc cancels');
    await page.getByRole('button', { name: '刪除' }).click();
    await page.getByRole('alertdialog').getByRole('button', { name: '刪除' }).click();
    await page.waitForSelector('.drawer:not(.open)');
    check(await page.evaluate(() => window.__mock.file.reminders.length) === 3, 'confirming the dialog deletes');

    // 分頁：一進來是「提醒」；假日、紀念日各在自己的分頁；右上角的新增按鈕跟著分頁換
    const selectedTab = () => page.locator('[role=tab][aria-selected=true]').getAttribute('id');
    check(await selectedTab() === 'tab-reminders' && !(await page.getByText('國定假日與補班').isVisible()) && await page.getByText('提醒方式').isVisible(), 'reminders tab comes first, holidays are not on it');
    await page.locator('#tab-days').click();
    check(await page.getByText('國定假日與補班').isVisible() && await page.getByRole('button', { name: /^新增/ }).count() === 0, 'holidays tab has no add button at the top');
    if (shotDir) await page.screenshot({ path: path.join(shotDir, 'settings-days-tab-light.png') });

    // 額外上班／放假日
    await page.locator('input[type=date][aria-label="日期"]').fill('2027-07-29');
    await page.getByRole('button', { name: '加入' }).click();
    check(await page.evaluate(() => window.__mock.file.dayOverrides['2027-07-29']) === 'holiday', 'extra day saved as a day off');
    check((await page.locator('.section', { hasText: '額外上班／放假日' }).innerText()).includes('放假'), 'extra day listed as 放假');

    // 國定假日與補班：一天一個開關，關掉 = 照平常；全部關閉／全部開啟；關掉的不會跑到上面「額外上班／放假日」
    const overrides = () => page.evaluate(() => window.__mock.file.dayOverrides);
    const daySwitch = (name) => page.getByRole('checkbox', { name });
    check(await page.locator('.list-year').allInnerTexts().then((t) => t.join()) === '2026 年,2027 年', 'official days are grouped by year');
    await daySwitch('12/25（五） 行憲紀念日').click({ force: true });
    await daySwitch('2/20（六） 補行上班').click({ force: true });
    const o = await overrides();
    check(o['2026-12-25'] === 'workday' && o['2027-02-20'] === 'holiday', 'turning a holiday off makes it a workday, a make-up day a day off');
    check(await page.locator('.day-row.off .tag-off').allInnerTexts().then((t) => t.join()) === '照常上班,照常放假', 'a day turned off says what happens instead');
    check(!(await page.locator('.section', { hasText: '額外上班／放假日' }).innerText()).includes('12/25'), 'days turned off stay out of the extra days list');
    if (shotDir) await page.locator('.section', { hasText: '國定假日與補班' }).screenshot({ path: path.join(shotDir, 'official-days-some-off-light.png') });
    await page.getByRole('button', { name: '全部關閉' }).click();
    check(Object.keys(await overrides()).length === 5 && await page.getByRole('button', { name: '全部關閉' }).isDisabled(), 'turn all off');
    if (shotDir) await page.locator('.section', { hasText: '國定假日與補班' }).screenshot({ path: path.join(shotDir, 'official-days-all-off-light.png') });
    await page.getByRole('button', { name: '全部開啟' }).click();
    check(JSON.stringify(Object.keys(await overrides())) === '["2027-07-29"]' && await page.getByRole('button', { name: '全部開啟' }).isDisabled(), 'turn all back on keeps only the own special day');

    // 左右鍵換分頁：從「紀念日」按右鍵到「假日」、再按左鍵回來
    await page.locator('#tab-anniversaries').click();
    await page.keyboard.press('ArrowRight');
    const afterRight = [await selectedTab(), await page.evaluate(() => document.activeElement?.id)];
    check(afterRight.join() === 'tab-days,tab-days', `arrow keys move between tabs and keep focus on the tab (${afterRight})`);
    await page.keyboard.press('ArrowLeft');
    check(await selectedTab() === 'tab-anniversaries' && await page.getByRole('button', { name: '新增紀念日' }).count() === 1 && await page.getByRole('button', { name: '新增提醒' }).count() === 0, 'anniversaries tab puts 新增紀念日 at the top');

    // 紀念日：列表看得到農曆日期、滿幾年、什麼時候提醒、還有幾天
    const momRow = page.locator('.list-row', { hasText: '媽媽生日' });
    const momText = await momRow.innerText();
    check(['農曆三月初五', '滿 61 年', '3 天前、當天 09:00', '12', '天後'].every((s) => momText.includes(s)), `anniversary row shows its lunar date, years, alerts and countdown (${momText.replace(/\n/g, ' ')})`);
    if (shotDir) await page.screenshot({ path: path.join(shotDir, 'settings-anniversaries-tab-light.png') });
    // 新增：國曆 2/29、2015 年、三個提醒裡刪掉一個、各自的時間
    await page.getByRole('button', { name: '新增紀念日' }).click();
    await page.waitForFunction(() => document.activeElement?.id === 'anniv-title');
    check(await page.locator('#anniv-save').isDisabled(), 'anniversary save disabled until named');
    await page.locator('#anniv-title').fill('結婚紀念日');
    await page.getByRole('button', { name: '農曆' }).click();
    check(await page.getByRole('combobox', { name: '月' }).locator('option').first().innerText() === '正月' && await page.getByRole('checkbox', { name: '閏月' }).isVisible(), 'lunar dates pick from 正月 and can be a leap month');
    await page.getByRole('button', { name: '國曆' }).click();
    await page.getByRole('combobox', { name: '月' }).selectOption('2');
    check(await page.getByRole('combobox', { name: '日' }).locator('option').count() === 29, 'February offers up to the 29th');
    await page.getByRole('combobox', { name: '日' }).selectOption('29');
    const yearField = page.locator('.field', { has: page.locator('#anniv-year') });
    if (shotDir) {
      // 年份欄位的每種樣子（淺色、深色）：空的、滑鼠移上去（出現上下箭頭）、打字中。
      for (const scheme of ['light', 'dark']) {
        await page.emulateMedia({ colorScheme: scheme });
        await yearField.screenshot({ path: path.join(shotDir, `year-empty-${scheme}.png`) });
        await page.locator('#anniv-year').hover();
        await yearField.screenshot({ path: path.join(shotDir, `year-hover-${scheme}.png`) });
        await page.locator('#anniv-year').fill('2015');
        await yearField.screenshot({ path: path.join(shotDir, `year-typing-${scheme}.png`) });
        await page.locator('#anniv-year').fill('');
      }
      await page.emulateMedia({ colorScheme: 'light' });
    }
    await page.locator('#anniv-year').fill('2015');
    await page.locator('#anniv-year').blur();
    await page.getByRole('combobox', { name: '第 1 個提醒提前幾天' }).selectOption('7');
    await page.getByRole('button', { name: '再加一個提醒' }).click();
    await page.getByLabel('第 3 個提醒幾點').fill('20:00');
    await page.getByRole('button', { name: '刪掉第 2 個提醒' }).click();
    const previewed = await page.waitForFunction(() => document.getElementById('anniv-preview')?.innerText.includes('滿 12 年'), null, { timeout: 3000 }).then(() => true, () => false);
    check(previewed && await page.locator('#anniv-preview .preview-row').count() === 3, 'editor previews the next times with years');
    if (shotDir) {
      await page.screenshot({ path: path.join(shotDir, 'anniversary-edit-light.png') });
      await page.evaluate(() => { const b = document.querySelector('.drawer-body'); b.scrollTop = b.scrollHeight; });
      await page.locator('.drawer').screenshot({ path: path.join(shotDir, 'anniversary-edit-bottom-light.png') });
    }
    await page.locator('#anniv-save').click();
    await page.waitForSelector('.drawer:not(.open)');
    const wedding = await page.evaluate(() => window.__mock.file.anniversaries.find((a) => a.title === '結婚紀念日'));
    check(JSON.stringify([wedding?.date, wedding?.year, wedding?.alerts]) === JSON.stringify([{ calendar: 'solar', month: 2, day: 29 }, 2015, [{ daysBefore: 7, time: '09:00' }, { daysBefore: 1, time: '20:00' }]]), `anniversary saved with its date, year and alerts (${JSON.stringify(wedding)})`);
    // 刪除要先確認
    await page.waitForFunction(() => document.getElementById('drawer').getBoundingClientRect().left >= window.innerWidth);
    await page.locator('.list-row', { hasText: '結婚紀念日' }).click();
    await page.locator('.drawer').getByRole('button', { name: '刪除' }).click();
    await page.getByRole('alertdialog').getByRole('button', { name: '刪除' }).click();
    await page.waitForSelector('.drawer:not(.open)');
    check(await page.evaluate(() => window.__mock.file.anniversaries.map((a) => a.title).join()) === '媽媽生日', 'anniversary deleted after confirming');
    await page.waitForFunction(() => document.getElementById('drawer').getBoundingClientRect().left >= window.innerWidth);
    // 紀念日分頁底下那行帶到「提醒」分頁的提醒方式
    await page.getByRole('button', { name: '去設定' }).click();
    check(await selectedTab() === 'tab-reminders' && await page.locator('#reminder-style').isVisible(), '去設定 goes to the reminders tab and its 提醒方式');

    // 桌寵：換一隻寫進設定
    await page.locator('.nav-item[data-page="pets"]').click();
    await page.locator('.pet-card', { hasText: '水豚（側面）' }).click();
    check(await page.evaluate(() => window.__mock.config.pet) === 'capybara-side', 'switching pet writes config');
    check((await page.locator('.pet-card', { hasText: '水豚' }).first().innerText()).includes('陪你 12 天'), 'pet card shows days together');
    // 取名字：寫進設定，卡片與側欄跟著換；清空就回到桌寵包自己的名字
    const nameBox = () => page.getByRole('textbox', { name: '名字' });
    check(await nameBox().getAttribute('placeholder') === '水豚（側面）' && await nameBox().inputValue() === '', 'name box shows the pet kind until named');
    await nameBox().fill('  圓圓 ');
    await nameBox().press('Enter');
    await page.locator('.pet-card', { hasText: '圓圓' }).waitFor();
    check(await page.evaluate(() => window.__mock.config.petNames?.['capybara-side']) === '圓圓' && await page.locator('#brand-name').innerText() === '圓圓', 'naming the pet writes config and shows the name');
    if (shotDir) await page.locator('.section', { hasText: '活潑程度' }).screenshot({ path: path.join(shotDir, `settings-pet-name-${scheme}.png`) });
    await nameBox().fill('');
    await nameBox().press('Enter');
    await page.locator('.pet-card', { hasText: '水豚（側面）' }).waitFor();
    check(await page.evaluate(() => !('capybara-side' in (window.__mock.config.petNames || {}))), 'clearing the name goes back to the pet kind');
    const userBox = page.getByRole('textbox', { name: '怎麼叫你' });
    await userBox.fill(' 小明 ');
    await userBox.press('Enter');
    await page.waitForFunction(() => window.__mock.config.userName === '小明');
    check(await userBox.inputValue() === '小明', 'what the pet calls you is saved trimmed');
    await page.getByRole('button', { name: '大' }).click();
    check(await page.evaluate(() => window.__mock.config.sizeDip) === 176, 'size choice writes config');
    await page.getByRole('button', { name: '安靜' }).click();
    await page.getByRole('button', { name: '避開這些地方' }).click();
    await page.getByRole('button', { name: '常講' }).click();
    await page.getByRole('checkbox', { name: '站到視窗上' }).click({ force: true });
    await page.getByRole('button', { name: '10 分' }).click();
    const cfg = await page.evaluate(() => window.__mock.config);
    check(cfg.activity === 'calm' && cfg.roam === 'avoid' && cfg.talk === 'often' && cfg.standOnWindows === false && cfg.sleepAfterSecs === 600, 'behavior settings write config');
    // 避開這些地方：列出畫好的框、可以刪、按新增請核心在螢幕上畫
    check(await page.getByText('600 × 140').isVisible(), 'forbidden areas are listed with their size');
    await page.getByRole('button', { name: '新增禁止範圍' }).click();
    check(await page.evaluate(() => window.__mock.calls.some((c) => c.cmd === 'drawArea' && c.kind === 'forbidden')), 'adding a forbidden area asks the core to draw one');
    // 滑鼠移到那一列：請核心在桌面上框出來；移開就收掉
    await page.locator('.setting', { hasText: '禁止範圍 1' }).hover();
    if (shotDir) await page.locator('.setting', { hasText: '禁止範圍 1' }).screenshot({ path: path.join(shotDir, 'settings-area-hover-light.png') });
    check(await page.evaluate(() => window.__mock.calls.some((c) => c.cmd === 'previewArea' && c.kind === 'forbidden' && c.area?.left === 100 && c.area?.bottom === 1040 && c.blink)), 'hovering a forbidden area previews it blinking on the desktop');
    check(await page.locator('.setting', { hasText: '禁止範圍 1' }).evaluate((el) => getComputedStyle(el).cursor) === 'pointer', 'an area row shows a pointing hand');
    const before = await page.evaluate(() => window.__mock.calls.length);
    await page.locator('.setting', { hasText: '禁止範圍 1' }).locator('.setting-title').click();
    check(await page.evaluate((n) => window.__mock.calls.slice(n).some((c) => c.cmd === 'previewArea' && c.blink), before), 'clicking an area row blinks it again');
    await page.getByRole('heading', { name: '行為' }).hover();
    check(await page.evaluate(() => { const c = window.__mock.calls.at(-1); return c.cmd === 'previewArea' && !c.area; }), 'leaving the row hides the preview');
    await page.getByRole('button', { name: '刪除禁止範圍 1' }).click();
    check(await page.evaluate(() => window.__mock.config.forbiddenAreas.length) === 0, 'deleting a forbidden area writes config');
    await page.getByRole('button', { name: '只在範圍內' }).click();
    await page.getByRole('button', { name: '畫範圍' }).click();
    check(await page.evaluate(() => window.__mock.calls.some((c) => c.cmd === 'drawArea' && c.kind === 'allowed')), 'drawing the allowed area asks the core');
    if (shotDir) await page.screenshot({ path: path.join(shotDir, 'settings-pets-light.png') });

    // 新增桌寵：選一張圖 → 看到整理好的樣子 → 取名字 → 完成就換過去
    const tinyPng = Buffer.from('iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+ip1sAAAAASUVORK5CYII=', 'base64');
    await page.getByRole('button', { name: '新增桌寵' }).click();
    check(await page.locator('#create-pet').isDisabled(), 'new pet: done disabled before picking an image');
    await page.locator('.drawer input[type=file]').setInputFiles({ name: 'cat.png', mimeType: 'image/png', buffer: tinyPng });
    await page.waitForSelector('img[alt="整理好的桌寵"]');
    if (shotDir) await page.screenshot({ path: path.join(shotDir, 'settings-newpet-light.png') });
    check(await page.locator('.piece-strip img').count() === 4, 'a four-pose sheet shows its four pieces before creating');
    await page.locator('#pet-name').fill('小白');
    await page.locator('#create-pet').click();
    await page.waitForSelector('#action-walk');
    check(await page.evaluate(() => window.__mock.config.pet) === 'my-1' && (await page.locator('#brand-name').innerText()) === '小白', 'new pet created from one image becomes the current pet and opens its action editor');

    // 動作編輯：一次加好幾張 → 出現速度；調快、看提示詞、刪一張、多一組待機
    const walk = page.locator('#action-walk');
    await walk.locator('.thumb.add input[type=file]').setInputFiles([{ name: 'w1.png', mimeType: 'image/png', buffer: tinyPng }, { name: 'w2.png', mimeType: 'image/png', buffer: tinyPng }]);
    await walk.locator('.thumb:not(.add)').nth(1).waitFor();
    check(await page.evaluate(() => window.__mock.calls.some((c) => c.cmd === 'add_frames' && c.state === 'walk' && c.count === 2)), 'several images can be added to an action at once');
    check(await page.evaluate(() => window.__mock.calls.some((c) => c.cmd === 'switchPet' && c.id === 'my-1')), 'changing frames reloads the current pet');
    await walk.getByRole('button', { name: '快' }).click();
    check(await page.evaluate(() => window.__mock.calls.some((c) => c.cmd === 'set_action_speed' && c.state === 'walk' && c.fps === 12)), 'speed buttons write a faster fps');
    await walk.getByRole('button', { name: '給 AI 的提示詞' }).click();
    check((await walk.locator('.prompt-text').innerText()).includes('走路'), 'prompt for the action is shown');
    await walk.getByRole('button', { name: '下載範例圖' }).click();
    check(await page.evaluate(() => window.__mock.calls.some((c) => c.cmd === 'save_example' && c.state === 'walk')), 'example image can be saved');
    if (shotDir) await page.screenshot({ path: path.join(shotDir, 'settings-actions-light.png'), fullPage: true });
    await walk.locator('.thumb:not(.add)').first().hover();
    await walk.getByRole('button', { name: '刪掉第 1 張' }).click();
    await page.waitForFunction(() => document.querySelectorAll('#action-walk .thumb:not(.add)').length === 1);
    check(true, 'a frame can be removed');
    await page.locator('#action-idle .add-group input[type=file]').setInputFiles({ name: 'idle2.png', mimeType: 'image/png', buffer: tinyPng });
    await page.waitForFunction(() => document.querySelectorAll('#action-idle .action-group').length === 2);
    check(await page.evaluate(() => window.__mock.calls.some((c) => c.cmd === 'add_frames' && c.state === 'idle' && c.variant === '2')), 'another idle group can be added');

    await page.getByRole('button', { name: '刪除這隻' }).click();
    check(await page.evaluate(() => window.__mock.pets.length) === 3 && await page.getByRole('alertdialog').getByText('救不回來').isVisible(), 'deleting a pet asks first');
    await page.getByRole('alertdialog').getByRole('button', { name: '刪除' }).click();
    await page.waitForSelector('.pet-grid');
    check(await page.evaluate(() => window.__mock.pets.length === 2 && window.__mock.config.pet !== 'my-1'), 'deleting the current custom pet switches to another one');
    check(await page.locator('.pet-cell', { hasText: '水豚（側面）' }).getByRole('button', { name: '編輯動作' }).count() === 0, 'built-in pets have no edit button');

    // 視窗歸位：送指令給核心
    await page.locator('.nav-item[data-page="windows"]').click();
    await page.getByRole('button', { name: '記住目前位置' }).click();
    check(await page.evaluate(() => window.__mock.calls.some((c) => c.cmd === 'saveLayout')), 'save layout sends command');
    await page.getByRole('checkbox', { name: '記住新視窗時說一聲' }).click({ force: true });
    check(await page.evaluate(() => window.__mock.config.announceNewWindows) === false, 'new-window announcement switch writes config');
    if (shotDir) await page.screenshot({ path: path.join(shotDir, 'settings-windows-light.png') });

    // 專注：開始專注送指令給核心；時間、倒數方式寫進設定
    await page.locator('.nav-item[data-page="focus"]').click();
    await page.getByRole('button', { name: '開始專注' }).click();
    check(await page.evaluate(() => window.__mock.calls.some((c) => c.cmd === 'focusStart')), 'start focus sends command');
    // 專注中：按鈕跟右鍵選單一樣變成「停止專注（還剩幾分）」
    await page.getByRole('button', { name: '停止專注（還剩 25 分）' }).click();
    check(await page.evaluate(() => window.__mock.calls.some((c) => c.cmd === 'focusStop')), 'stop focus sends command');
    await page.getByRole('button', { name: '開始專注' }).waitFor();
    await page.getByRole('button', { name: '45 分' }).click();
    await page.getByRole('button', { name: '滑鼠移過去才顯示' }).click();
    check(await page.evaluate(() => { const c = window.__mock.config; return c.focusMinutes === 45 && c.focusCountdown === 'hover'; }), 'focus length and countdown mode write config');
    if (shotDir) await page.screenshot({ path: path.join(shotDir, 'settings-focus-light.png') });

    // 傳話紀錄：照來源分組、打開就算看過、還沒按完成的確認框可以從這裡按完成、清除送指令給核心
    await page.locator('.nav-item[data-page="relay"]').click();
    await page.locator('.section h2', { hasText: 'Claude' }).waitFor();
    check(JSON.stringify(await page.locator('.section h2').allInnerTexts()) === JSON.stringify(['Claude', '沒有署名']), 'relay log groups by sender, latest sender first');
    check(await page.evaluate(() => window.__mock.calls.some((c) => c.cmd === 'relayLogSeen')), 'opening the relay log marks it seen');
    check(await page.locator('.page-hint code').innerText() === 'Youban.exe --relay-help', 'relay page points to the full usage');
    check(await page.locator('.list-row', { hasText: '建置完成' }).getByRole('button', { name: '完成' }).count() === 0, 'notices have no done button');
    if (shotDir) await page.screenshot({ path: path.join(shotDir, 'settings-relay-light.png') });
    await page.locator('.list-row', { hasText: 'PR 等你審' }).getByRole('button', { name: '完成' }).click();
    check(await page.evaluate(() => window.__mock.calls.some((c) => c.cmd === 'relayDone' && c.key === '#2')), 'done in the relay log is sent to the core');
    await page.locator('.list-row', { hasText: 'PR 等你審' }).getByRole('button', { name: '完成' }).waitFor({ state: 'detached' });
    await page.locator('.section', { hasText: '沒有署名' }).getByRole('button', { name: '清除' }).click();
    check(await page.evaluate(() => window.__mock.calls.some((c) => c.cmd === 'relayLogClear' && 'from' in c && c.from === null)), 'clearing one sender sends that sender');
    await page.locator('.section h2', { hasText: '沒有署名' }).waitFor({ state: 'detached' });
    await page.getByRole('button', { name: '全部清除' }).click();
    await page.locator('.empty-title', { hasText: '還沒有人傳話給你' }).waitFor();
    check(await page.evaluate(() => window.__mock.calls.some((c) => c.cmd === 'relayLogClear' && !('from' in c))), 'clear all sends no sender');
    await page.locator('.nav-item[data-page="windows"]').click();

    // 點一個視窗直接打數字改位置；寬高空著不能套用
    await page.locator('.list-row', { hasText: '記事本' }).click();
    await page.locator('#win-left:focus').waitFor();
    await page.locator('#win-height').fill('');
    check(await page.getByRole('button', { name: '套用' }).isDisabled(), 'empty window size cannot be applied');
    await page.locator('#win-height').fill('720');
    await page.locator('#win-left').fill('-8');
    if (shotDir) await page.screenshot({ path: path.join(shotDir, 'settings-window-edit-light.png') });
    await page.getByRole('button', { name: '套用' }).click();
    check(await page.evaluate(() => window.__mock.calls.some((c) => c.cmd === 'editWindow' && c.id === 4242 && c.left === -8 && c.top === 50 && c.width === 800 && c.height === 720)), 'editing a window sends its new position');
    // 抽屜滑出去之前還蓋在右半邊，下面的 force click 會點到它。
    await page.waitForFunction(() => document.getElementById('drawer').getBoundingClientRect().left >= window.innerWidth);

    // 一般：開機啟動失敗要顯示原因、開關回到關
    await page.locator('.nav-item[data-page="general"]').click();
    // 用 click 不用 check：失敗後開關會彈回「關」，check() 會一直等它變成「開」。
    await page.getByRole('checkbox', { name: '開機自動啟動' }).click({ force: true });
    await page.waitForSelector('.setting-error');
    check((await page.locator('.setting-error').innerText()).includes('寫不進登錄'), 'autostart failure shows reason');
    check(!(await page.getByRole('checkbox', { name: '開機自動啟動' }).isChecked()), 'autostart switch reverts');

    await page.getByRole('checkbox', { name: '自動更新' }).click({ force: true });
    check(await page.evaluate(() => window.__mock.config.autoUpdate) === false && await page.getByText('目前版本 0.1.0').isVisible(), 'auto update switch writes config and shows version');
    check((await page.locator('.sidebar-foot').innerText()).includes('0.1.0'), 'sidebar shows the version');

    // 外觀：系統是淺色時選「深色」整個畫面變深、寫進設定；選回「跟著系統」又變淺
    const rgb = (hex) => `rgb(${[1, 3, 5].map((i) => parseInt(hex.slice(i, i + 2), 16)).join(', ')})`;
    const pageBg = () => page.evaluate(() => getComputedStyle(document.body).backgroundColor);
    await page.getByRole('button', { name: '深色' }).click();
    check(await page.evaluate(() => window.__mock.config.colorScheme) === 'dark' && await pageBg() === rgb(tokens.color.dark.surface), 'picking dark writes config and turns the page dark on a light system');
    if (shotDir) await page.screenshot({ path: path.join(shotDir, 'settings-general-forced-dark.png') });
    await page.getByRole('button', { name: '跟著系統' }).click();
    check(await page.evaluate(() => window.__mock.config.colorScheme) === 'system' && await pageBg() === rgb(tokens.color.light.surface), 'following the system turns it light again');

    // 檢查更新：查到新版、下載好 → 按鈕變成「現在更新」，按下去叫核心換新版
    await page.getByRole('button', { name: '檢查更新' }).click();
    await page.getByRole('button', { name: '現在更新' }).waitFor();
    check(await page.getByText('新版 0.2.0 準備好了').isVisible(), 'check for updates shows the downloaded version');
    if (shotDir) await page.screenshot({ path: path.join(shotDir, 'settings-update-ready-light.png') });
    await page.getByRole('button', { name: '現在更新' }).click();
    check(await page.evaluate(() => window.__mock.calls.some((c) => c.cmd === 'apply_update')), 'update now asks the core to apply it');

    if (shotDir) await page.screenshot({ path: path.join(shotDir, 'settings-general-light.png') });

    // 功能開關在功能頁的標題旁：關掉提醒 → 那頁只剩關閉狀態、側邊欄變淡；同一個開關開回來
    await page.locator('.nav-item[data-page="reminders"]').click();
    await page.getByRole('checkbox', { name: '提醒', exact: true }).click({ force: true });
    check(await page.evaluate(() => window.__mock.config.features.reminders) === false, 'turning a feature off writes config');
    check(await page.getByText('提醒已關閉').isVisible() && !(await page.getByRole('button', { name: '新增提醒' }).count()), 'turned-off feature page shows only the off state');
    check(await page.locator('.nav-item[data-page="reminders"].off').count() === 1, 'turned-off feature is dimmed in the sidebar');
    if (shotDir) await page.screenshot({ path: path.join(shotDir, 'settings-feature-off-light.png') });
    await page.getByRole('checkbox', { name: '提醒', exact: true }).click({ force: true });
    check(await page.evaluate(() => window.__mock.config.features.reminders) === true && await page.getByText('喝水').isVisible(), 'feature turned back on from its page');
    check(await page.locator('.nav-item.off').count() === 0, 'sidebar no longer dimmed');
    await page.locator('.nav-item[data-page="general"]').click();

    // 核心沒在跑：橫幅出現、要核心的按鈕停用
    await page.evaluate(() => { window.__mock.running = false; });
    await page.waitForSelector('#core-banner:not([hidden])', { timeout: 5000 });
    await page.locator('.nav-item[data-page="windows"]').click();
    check(await page.getByRole('button', { name: '記住目前位置' }).isDisabled(), 'core-dependent button disabled when core stopped');
  }

  await page.locator('.nav-item[data-page="reminders"]').click();
  if (shotDir) await page.screenshot({ path: path.join(shotDir, `settings-reminders-${scheme}.png`) });
  if (shotDir) {
    await page.locator('#tab-days').click();
    await page.screenshot({ path: path.join(shotDir, `settings-days-${scheme}.png`) });
    const official = page.locator('.section', { hasText: '國定假日與補班' });
    await official.screenshot({ path: path.join(shotDir, `official-days-${scheme}.png`) });
    await page.locator('#tab-anniversaries').click();
    await page.locator('#tab-days').hover();
    await page.screenshot({ path: path.join(shotDir, `settings-anniversaries-${scheme}.png`) });
    const anniversaries = page.locator('.tab-panel');
    await anniversaries.screenshot({ path: path.join(shotDir, `anniversaries-${scheme}.png`) });
    // 其他樣子（只截圖用）：今天就是、暫停中、一筆都沒有。
    await page.evaluate(() => {
      state.file.anniversaries.push({ ...structuredClone(state.file.anniversaries[0]), id: 'paused', title: '爸爸生日', enabled: false });
      state.anniversaryInfo.paused = { ...state.anniversaryInfo.mom };
      state.anniversaryInfo.mom = { ...state.anniversaryInfo.mom, daysLeft: 0 };
      render();
    });
    await anniversaries.screenshot({ path: path.join(shotDir, `anniversaries-today-paused-${scheme}.png`) });
    await page.evaluate(() => { state.file.anniversaries = []; render(); });
    await anniversaries.screenshot({ path: path.join(shotDir, `anniversaries-empty-${scheme}.png`) });
    await page.locator('#tab-days').click();
    await official.getByRole('checkbox').first().click({ force: true });
    await official.getByRole('button', { name: '全部開啟' }).hover();
    await official.screenshot({ path: path.join(shotDir, `official-days-one-off-hover-${scheme}.png`) });
  }
  check(errors.length === 0, `no page errors (${scheme}) ${errors.join(' | ')}`);
  await ctx.close();
}
await browser.close();
server.close();
console.log(failures ? `${failures} failed` : 'all passed');
process.exit(failures ? 1 : 0);
