function openFiles() {
  return new Promise((resolve, reject) => {
    const request = indexedDB.open('windao-quota-files', 1);
    request.onupgradeneeded = () => request.result.createObjectStore('handles');
    request.onsuccess = () => resolve(request.result);
    request.onerror = () => reject(request.error);
  });
}

async function fileRecord(id, action, handle) {
  const db = await openFiles();
  try {
    return await new Promise((resolve, reject) => {
      const tx = db.transaction('handles', action === 'get' ? 'readonly' : 'readwrite');
      const store = tx.objectStore('handles');
      let request;
      if (action === 'set') request = store.put(handle, id);
      else if (action === 'remove') request = store.delete(id);
      else if (action === 'clear') request = store.clear();
      else request = store.get(id);
      tx.oncomplete = () => resolve(request.result);
      tx.onerror = tx.onabort = () => reject(tx.error);
    });
  } finally { db.close(); }
}

async function writeReport(handle, current, repeated) {
  if (await handle.queryPermission({ mode: 'readwrite' }) !== 'granted') throw Error('文件写入权限已撤销。');
  const file = await handle.getFile();
  if (file.size > 0) {
    if (file.size > 128 * 1024) throw Error('请选择专用于该账号的 JSON 文件。');
    let saved;
    try { saved = JSON.parse(await file.text()); } catch { throw Error('请选择空文件或本工具保存的账号文件。'); }
    const owner = saved?.windaoShare;
    if (!owner || owner.source !== current.source || !current.account || owner.account !== current.account) {
      throw Error('文件中的账号与当前页面无法匹配，请新建单独的文件。');
    }
  } else if (repeated) throw Error('账号文件已更换或清空。');
  const stream = await handle.createWritable();
  try {
    const report = { ...current.report, windaoShare: { source: current.source, account: current.account } };
    await stream.write(JSON.stringify(report, null, 2));
    await stream.close();
  } catch (e) {
    await stream.abort().catch(() => {});
    throw e;
  }
}
