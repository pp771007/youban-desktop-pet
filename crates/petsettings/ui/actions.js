'use strict';

// 「動作編輯」用的動作清單：每個動作叫什麼、一句話提示、給 AI 的姿勢描述。
// 狀態 id 跟 petkit::states 同名；分組照使用者平常看到的頻率排。

const ACTION_GROUPS = [
  { title: '最常看到', items: ['idle', 'walk', 'sleep'] },
  { title: '跟你互動', items: ['happy', 'notify', 'drag', 'fall', 'land'] },
  { title: '小動作', items: ['look', 'sit', 'yawn', 'doze', 'wake', 'blink', 'hop', 'climb'] },
];

const ACTIONS = {
  idle: { label: '站著', hint: '平常發呆，可以有好幾組', count: 2, pose: '正面站著、身體放鬆、眼睛張開；第二張同姿勢但身體微微起伏（呼吸）' },
  walk: { label: '走路', hint: '側面朝右，腳一前一後', count: 4, pose: '正側面、面向右、慢慢走路：① 前腳往前踏 ② 四腳靠攏、身體在最高點 ③ 另一邊的腳往前踏 ④ 再次靠攏' },
  sleep: { label: '睡覺', hint: '趴著睡，兩張做出呼吸', count: 2, pose: '趴著睡覺、眼睛閉著；第二張同一個睡姿、身體微微鼓起（吸氣）' },
  happy: { label: '開心', hint: '被點一下、被摸頭時', count: 2, pose: '正面、舉起手、眼睛笑成彎月；第二張開心地跳起離地' },
  notify: { label: '有提醒', hint: '叫你看提醒，例如搖鈴', count: 2, pose: '正面站著、一隻手拿小鈴鐺在低處搖；第二張同一隻手把鈴鐺舉高、嘴巴張開像在喊' },
  drag: { label: '被拎起來', hint: '懸空、手腳往下垂', count: 2, pose: '被人從背上拎起來懸空、手腳自然下垂；第二張手腳在空中划動' },
  fall: { label: '掉下來', hint: '在空中、手腳張開', count: 1, pose: '從空中往下掉、手腳張開、表情驚訝' },
  land: { label: '落地', hint: '壓扁一點緩衝', count: 1, pose: '剛落地、身體壓扁一點、腳彎曲緩衝' },
  look: { label: '張望', hint: '轉頭看旁邊', count: 1, pose: '站著、頭轉向側邊張望' },
  sit: { label: '坐下', hint: '坐著休息', count: 1, pose: '坐著、屁股著地、手放在肚子上' },
  yawn: { label: '打哈欠', hint: '嘴巴張大', count: 1, pose: '站著打大哈欠、眼睛瞇成一線' },
  doze: { label: '打瞌睡', hint: '站著頭往前點', count: 1, pose: '站著打瞌睡、頭往前垂、眼睛閉著' },
  wake: { label: '伸懶腰', hint: '前腳往前伸', count: 1, pose: '前腳往前伸、屁股翹高伸懶腰' },
  blink: { label: '眨眼', hint: '跟站著一模一樣，只閉眼', count: 1, pose: '跟附圖完全相同的站姿與大小，只有眼睛閉起來' },
  hop: { label: '小跳', hint: '原地輕輕跳', count: 2, pose: '原地蹲低準備跳；第二張輕輕跳起離地' },
  climb: { label: '爬牆', hint: '直立、抓著右邊的牆', count: 2, pose: '側面、身體直立、手腳抓著右邊的牆往上爬（牆不用畫）、面向右；第二張換另一組手腳往上' },
  dizzy: { label: '頭暈', hint: '眼睛轉圈圈、站不穩', count: 2, pose: '站著搖搖晃晃、兩隻眼睛變成轉圈圈的漩渦、嘴巴歪一邊、身體往左歪；第二張同樣的臉、身體往右歪' },
};

/** 給 AI 的提示詞：附上自己的角色圖＋範例圖一起丟。 */
function actionPrompt(state) {
  const a = ACTIONS[state];
  const layout = a.count === 1 ? '畫 1 個姿勢' : `一張圖裡畫 ${a.count} 個姿勢，由左到右排成一列（4 個可以排成 2×2，順序左上、右上、左下、右下）`;
  return [
    `第一張附圖是我的桌面寵物角色。請畫出它「${a.label}」的動作，${layout}：`,
    a.pose,
    '',
    '規則：',
    '- 角色跟第一張附圖完全一樣：比例、配色、輪廓線粗細、畫風',
    '- 每個姿勢的角色大小一致、腳底在同一個高度',
    '- 姿勢之間留大片空白分開，不要畫格線、邊框、編號、文字，也不要畫問號、驚嘆號、速度線這類符號',
    '- 真正的透明背景 PNG（帶 alpha 通道），不要白底、不要灰白棋盤格、不要地面陰影',
    '- 第二張附圖是動作範例，只參考姿勢，不要照著畫它的角色',
  ].join('\n');
}
