const result = JSON.parse(decodeURIComponent(location.hash.slice(1)));
const save = document.getElementById('save');
const info = document.getElementById('info');
const status = document.getElementById('status');
const labels = { GXU_WEEKLY: '本周', GXU_FIVE_HOURLY: '5 小时' };

function handles() {
  return new Promise((resolve, reject) => {
    const req = indexedDB.open('windao-share', 1);
    req.onupgradeneeded = () => req.result.createObjectStore('files');
    req.onsuccess = () => resolve(req.result);
    req.onerror = () => reject(req.error);
  });
}

async function previous() {
  const db = await handles();
  return new Promise((resolve, reject) => {
    const req = db.transaction('files').objectStore('files').get(result.source);
    req.onsuccess = () => resolve(req.result);
    req.onerror = () => reject(req.error);
  }).finally(() => db.close());
}

async function remember(handle) {
  const db = await handles();
  return new Promise((resolve, reject) => {
    const tx = db.transaction('files', 'readwrite');
    tx.objectStore('files').put(handle, result.source);
    tx.oncomplete = resolve;
    tx.onerror = () => reject(tx.error);
  }).finally(() => db.close());
}

async function display() {
  if (result.error) { info.textContent = result.error; return; }
  document.getElementById('title').textContent = result.source + ' 用量';
  info.textContent = result.source === 'Gemini'
    ? result.report.pools.map(p => labels[p.poolType] + '已用 ' + Math.round(p.percentageUsed * 10000) / 100 + '%').join('\n')
    : '本周已用 ' + result.report.usagePercent + '%';
  let handle = await previous().catch(() => null);
  if (handle) save.textContent = '更新 ' + handle.name;
  save.hidden = false;
  save.onclick = async () => {
    try {
      if (!handle || await handle.requestPermission({ mode: 'readwrite' }) !== 'granted') {
        handle = await showSaveFilePicker({ suggestedName: result.source.toLowerCase() + '-usage.json', types: [{ description: 'WinDao 用量', accept: { 'application/json': ['.json'] } }] });
      }
      const stream = await handle.createWritable();
      await stream.write(JSON.stringify(result.report, null, 2));
      await stream.close();
      await remember(handle);
      save.textContent = '更新 ' + handle.name;
      status.textContent = '已保存。首次使用请在 WinDao 的对应服务中选择这份文件。';
    } catch (error) {
      status.textContent = error.name === 'AbortError' ? '已取消保存。' : '保存失败，请检查文件访问权限。';
    }
  };
}
display();
