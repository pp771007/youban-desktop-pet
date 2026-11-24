#!/usr/bin/env node
/*
 * release-guard.mjs —— PreToolUse(Bash / PowerShell) hook，跟著這個 repo 走。
 *
 * 這個 repo 的性質：推一個版號 tag，GitHub Actions 就發 Release，所有開著自動更新的玩家
 * （安裝版、免安裝版）一天內自動換上。指令本身回得來，後果回不來；已發布的 tag 也不該再動。
 * 所以推 tag 之前要先把「版號、給玩家的更新說明全文、會影響誰」講給使用者聽，等他同意。
 *
 * 擋：推版號 tag、`git push --tags`、推 `refs/tags/`、`git tag -f` 搬動既有 tag、
 *     `gh release create / delete / upload`、用 API 建立或刪除 tag / release。
 * 不擋：純本機 `git tag -a v1.2.3`（`git tag -d` 就沒了）、一般 push、查詢。
 *
 * 逃生門：指令尾端加註解 `#user-approved`。機器上有 `~/.claude/hooks/user-approval.mjs`
 * 就再加一道：使用者本人要同意過「這一條」（標記自己打得出來，只看標記守不住）。
 * 沒有那支（別人 clone 下來）就退回只看標記。判準只維護那一份，這裡不抄第二份。
 *
 * 這是盡力而為的黑名單：用變數拼網址一樣繞得過。真正的防線是「發版前先講給使用者聽」，
 * 這支只是確保不會忘。
 *
 * settings.json 的路徑佔位符要寫成有大括號的那種：有 args 時不經過 shell，沒大括號的寫法
 * 不會被展開，node 找不到檔案就 exit 1，而 hook 只認 exit 2 為「擋下」——寫錯會靜靜地不生效。
 */
import { readFileSync } from 'node:fs';
import { homedir } from 'node:os';
import { join } from 'node:path';
import { pathToFileURL } from 'node:url';

let input = {};
try { input = JSON.parse(readFileSync(0, 'utf8') || '{}'); } catch { process.exit(0); }
if (!/^(Bash|PowerShell)$/.test(input.tool_name || '')) process.exit(0);

const globalHook = async (name) => {
  try {
    return await import(pathToFileURL(join(homedir(), '.claude', 'hooks', name)).href);
  } catch {
    return null;
  }
};
const approval = await globalHook('user-approval.mjs');

const cmd = input.tool_input?.command || '';
const tagged = /#\s*user-approved/i.test(cmd);
if (tagged && (!approval || approval.userApproved(input))) process.exit(0);

const RELEASE_ASK = '要發的版號、給玩家的更新說明全文（會直接變成 Release 的說明）、會影響誰（開著自動更新的玩家一天內會換上）';
const RELEASE_WHY = '這是發版 —— 推上去 GitHub Actions 就發 Release，玩家的柚伴會自動更新，發了就不該再動';
const WRITE_METHOD = String.raw`(-X\s*"?(POST|PUT|PATCH|DELETE)|--method\s+"?(POST|PUT|PATCH|DELETE)|-Method\s+"?(Post|Put|Patch|Delete))`;
const RELEASE_API = String.raw`\/(releases|git\/refs\/tags|git\/tags)\b`;

// `exes`：這條規則只看開頭是這些程式的那一段（`git commit -m "…gh release create…"` 不算發版）。
const API_EXES = ['curl', 'gh', 'invoke-restmethod', 'invoke-webrequest', 'irm', 'iwr'];
const RULES = [
  { exes: ['git'], re: /\bgit\s+tag\b[^\n|&;]*\s(-f\b|--force\b)/, what: 'git tag -f（搬動既有 tag）', why: '把已經發布的版號搬到別的 commit —— 版號一樣、內容不同，外觀看不出來', ask: '要改內容就發下一個版號，不要搬動舊的' },
  { exes: ['git'], re: /\bgit\s+push\b[^\n|&;]*--tags\b/, what: 'git push --tags', why: RELEASE_WHY, ask: RELEASE_ASK },
  { exes: ['git'], re: /\bgit\s+push\b[^\n|&;]*refs\/tags\//, what: '推送 tag ref', why: `${RELEASE_WHY}（或刪掉遠端 tag）`, ask: RELEASE_ASK },
  { exes: ['git'], re: /\bgit\s+push\b[^\n|&;]*\s:?v\d+(\.\d+)+/, what: '推送版號 tag', why: RELEASE_WHY, ask: RELEASE_ASK },
  { exes: ['gh'], re: /\bgh\s+release\s+(create|delete|upload)\b/, what: 'gh release create / delete / upload', why: RELEASE_WHY, ask: RELEASE_ASK },
  { exes: API_EXES, re: new RegExp(`${WRITE_METHOD}[^\\n]*${RELEASE_API}`, 'i'), what: '用 API 建立 / 刪除 tag 或 release', why: `${RELEASE_WHY}（走 HTTP 而不是 git）`, ask: RELEASE_ASK },
  { exes: API_EXES, re: new RegExp(`${RELEASE_API}[^\\n]*${WRITE_METHOD}`, 'i'), what: '用 API 建立 / 刪除 tag 或 release', why: `${RELEASE_WHY}（走 HTTP 而不是 git）`, ask: RELEASE_ASK },
];

// 引號裡有空白的是一段話（commit 訊息、說明），拿掉；沒有空白的（"v0.8.0"、網址）留下引號裡的字。
const dropQuotedProse = (s) => s.replace(/'([^']*)'|"([^"]*)"/g, (_, a, b) => (/\s/.test(a ?? b) ? '""' : (a ?? b)));

// 有共用的切段工具就逐段看；沒有（別人 clone 下來）就整行掃 —— 多擋看得見，漏擋看不見。
const segments = await globalHook('command-segments.mjs');
const pieces = segments
  ? segments.splitSegments(segments.stripHeredocs(cmd, input.tool_name), input.tool_name)
    .map((seg) => ({ exe: segments.leadingExe(seg), text: dropQuotedProse(seg) }))
  : [{ exe: null, text: cmd }];
const hit = RULES.find((r) => pieces.some((p) => (p.exe === null || r.exes.includes(p.exe)) && r.re.test(p.text)));
if (!hit) process.exit(0);

(await globalHook('hook-log.mjs'))?.logBlock({ hook: 'youban-release-guard', tool: input.tool_name, what: hit.what, tagged, command: cmd.slice(0, 300) });

process.stderr.write(
  `⛔ [youban-desktop-pet] 擋下發版動作：${hit.what}\n` +
  `原因：${hit.why}。\n` +
  `先跟使用者講清楚：${hit.ask}，然後停下來等他回覆。\n` +
  (approval ? approval.APPROVAL_HOW
    : '使用者同意後，在指令尾端加註解 #user-approved 再下一次即可放行（沒得到同意不可以自己加）。\n'),
);
process.exit(2);
