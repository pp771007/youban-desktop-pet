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
  const db = {
    config: { pet: 'capybara', sizeDip: 128, sleepAfterSecs: 300, activity: 'normal', roam: 'screen', talk: 'sometimes', standOnWindows: true, hideInFullscreen: true, reminderSound: true, remindInFullscreen: false, autoUpdate: true, announceNewWindows: true, restReminder: true, restEveryMinutes: 50, restStyle: 'auto', focusMinutes: 25, shortBreakMinutes: 5, longBreakMinutes: 15, focusRoundsBeforeLong: 4, focusAutoContinue: true, focusStyle: 'auto', focusCountdown: 'always', allowedArea: null, forbiddenAreas: [{ left: 100, top: 900, right: 700, bottom: 1040 }], features: { reminders: true, windowRestore: true, focus: true } },
    file: {
      reminders: [
        { id: 'a', title: '喝水', enabled: true, time: '14:00', repeat: { kind: 'daily' }, holiday: 'normal', startDate: '2026-01-01' },
        { id: 'b', title: '繳卡費', enabled: false, time: '10:30', repeat: { kind: 'monthly', rule: { kind: 'day', day: 25 } }, holiday: 'prevWorkday', startDate: '2026-01-01' },
      ],
      dayOverrides: {},
    },
    calls: [],
    running: true,
    updateSteps: [],
  };
  const png = 'data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+ip1sAAAAASUVORK5CYII=';
  db.pets = [{ id: 'capybara', name: '水豚', image: png, editable: false, days: 12 }, { id: 'capybara-side', name: '水豚（側面）', image: png, editable: false, days: 0 }];
  const allStates = ['idle', 'walk', 'drag', 'fall', 'sleep', 'notify', 'blink', 'happy', 'sit', 'land', 'yawn', 'wake', 'talk', 'hop', 'look', 'doze', 'climb', 'dizzy'];
  db.actions = { groups: [{ state: 'idle', variant: 'default', frames: [png] }], fps: Object.fromEntries(allStates.map((s) => [s, 8])), defaultFps: Object.fromEntries(allStates.map((s) => [s, 8])) };
  const handlers = {
    initial_page: () => null,
    tokens: () => tokens,
    constants: () => ({ sizes: [96, 128, 176], version: '0.1.0' }),
    load_config: () => structuredClone(db.config),
    save_config: ({ config }) => { db.config = structuredClone(config); },
    load_reminders: () => structuredClone(db.file),
    save_reminders: ({ file }) => { db.file = structuredClone(file); },
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
      // 核心查新版：每讀一次進度往前走一步，停在最後一個。
      if (command.cmd === 'checkUpdate') db.updateSteps = [{ state: 'checking' }, { state: 'downloading', version: '0.2.0' }, { state: 'ready', version: '0.2.0' }];
      if (command.cmd === 'focusStart') db.focusStopLabel = '停止專注（還剩 25 分）';
      if (command.cmd === 'focusStop') db.focusStopLabel = null;
    },
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
    check(await page.evaluate(() => { const c = window.__mock.config; return c.restEveryMinutes === 30 && c.restStyle === 'card'; }), 'rest reminder interval and card style write config');
    await page.getByRole('checkbox', { name: '休息提醒' }).click({ force: true });
    check(await page.evaluate(() => window.__mock.config.restReminder) === false && !(await page.getByText('連續用多久').isVisible()), 'turning the rest reminder off hides its options');
    await page.getByRole('checkbox', { name: '全螢幕時照樣跳出來' }).click({ force: true });
    check(await page.evaluate(() => window.__mock.config.remindInFullscreen) === true, 'remind-in-fullscreen switch writes config');

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
    check(stretch && stretch.repeat.kind === 'interval' && stretch.repeat.everyMinutes === 90 && stretch.time === '09:00' && stretch.repeat.until === '18:00' && stretch.style === 'auto', 'template fills an interval reminder that goes away by itself');
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

    // 特別日子
    await page.locator('input[type=date][aria-label="日期"]').fill('2027-07-29');
    await page.getByRole('button', { name: '加入' }).click();
    check(await page.evaluate(() => window.__mock.file.dayOverrides['2027-07-29']) === 'holiday', 'special day saved as holiday');

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
    check(await page.evaluate(() => window.__mock.calls.some((c) => c.cmd === 'previewArea' && c.kind === 'forbidden' && c.area?.left === 100 && c.area?.bottom === 1040)), 'hovering a forbidden area previews it on the desktop');
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
  check(errors.length === 0, `no page errors (${scheme}) ${errors.join(' | ')}`);
  await ctx.close();
}
await browser.close();
server.close();
console.log(failures ? `${failures} failed` : 'all passed');
process.exit(failures ? 1 : 0);
