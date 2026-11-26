const id = location.hash.slice(1);
const $ = key => document.getElementById(key);
const send = (op, automatic = false) => chrome.runtime.sendMessage({ id, op, automatic });
const labels = { GXU_WEEKLY: '本周', GXU_FIVE_HOURLY: '5 小时' };
let record;

function display(next) {
  record = next;
  if (!record || record.error) {
    $('info').textContent = record?.error || '请重新在用量页开启分享。';
    $('save').hidden = $('stop').hidden = $('options').hidden = true;
    return;
  }
  const { source, report, account } = record.data;
  $('title').textContent = source + ' 用量';
  $('info').textContent = source === 'Gemini'
    ? report.pools.map(p => labels[p.poolType] + '已用 ' + Math.round(p.percentageUsed * 10000) / 100 + '%').join('\n')
    : '本周已用 ' + report.usagePercent + '%';
  $('save').hidden = !!record.automatic;
  $('stop').hidden = !record.automatic;
  $('options').hidden = !account || !!record.automatic;
  $('status').textContent = [record.note, record.automatic ? '每 5 分钟更新。可在这里停用，或关闭原用量页。' :
    !account ? '当前页面无法确认唯一账号，仅支持选择新文件保存本次读数。' : '请为每个账号选择单独的文件，并在 WinDao 中选择同一文件。'].filter(Boolean).join(' ');
}
$('save').onclick = async () => {
  try {
    const { source, account } = record.data;
    const handle = await showSaveFilePicker({
      suggestedName: source.toLowerCase() + '-' + (account?.slice(0, 8) || Date.now()) + '-usage.json',
      types: [{ description: 'WinDao 用量', accept: { 'application/json': ['.json'] } }],
    });
    await fileRecord(id, 'set', handle);
    display(await send('save', $('automatic').checked && !!account));
  } catch (e) {
    $('status').textContent = e.name === 'AbortError' ? '已取消保存。' : '无法保存，请检查所选文件的权限。';
  }
};
$('stop').onclick = async () => { display(await send('stop')); };
chrome.storage.onChanged.addListener((changes, area) => {
  if (area === 'session' && changes[id]) display(changes[id].newValue);
});
send('info').then(display).catch(() => display(null));
