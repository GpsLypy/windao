importScripts('quota.js', 'files.js');

const prefix = 'quota-';
const session = chrome.storage.session;

async function state(id) { return (await session.get(id))[id]; }
async function persist(id, record) { await session.set({ [id]: record }); }

async function sample(tabId, documentId) {
  const target = documentId ? { tabId, documentIds: [documentId] } : { tabId };
  const [frame] = await chrome.scripting.executeScript({ target, func: readQuota });
  if (!frame?.result || frame.result.error) throw Error(frame?.result?.error || '用量页面已关闭或无法访问。');
  return { data: frame.result, documentId: frame.documentId };
}

async function stop(id, record, note) {
  record.automatic = false;
  record.note = note;
  await persist(id, record);
  await chrome.alarms.clear(id);
  await fileRecord(id, 'remove');
}

function sameAccount(initial, current, automatic) {
  return initial.source === current.source && initial.account === current.account && (!automatic || !!current.account);
}

async function refresh(id, record, automatic) {
  const { data } = await sample(record.tabId, record.documentId);
  if (!sameAccount(record.data, data, automatic)) throw Error('页面账号已改变或无法确认。');
  const handle = await fileRecord(id, 'get');
  if (!handle) throw Error('请选择本次分享的账号文件。');
  await writeReport(handle, data, record.hasSaved);
  record.data = data;
  record.hasSaved = true;
  record.name = handle.name;
  record.note = '已保存到 ' + handle.name + '。';
  await persist(id, record);
}

chrome.action.onClicked.addListener(async tab => {
  const existing = Object.entries(await session.get(null)).find(([,r]) => r.automatic && r.tabId === tab.id);
  if (existing) {
    const [id, record] = existing;
    if (record.controlTabId) {
      try { await chrome.tabs.update(record.controlTabId, { active: true }); return; } catch { /* control was closed */ }
    }
    const control = await chrome.tabs.create({ url: chrome.runtime.getURL('save.html#' + id) });
    await navigator.locks.request(id, async () => {
      const latest = await state(id);
      if (latest) await persist(id, { ...latest, controlTabId: control.id });
    });
    return;
  }
  let record;
  try {
    const page = new URL(tab.url);
    if (!['https://gemini.google.com', 'https://cursor.com'].includes(page.origin)) throw Error('请在官方用量页使用此工具。');
    const { data, documentId } = await sample(tab.id);
    record = { tabId: tab.id, documentId, data, automatic: false, hasSaved: false };
  } catch (error) { record = { error: error.message }; }
  const id = prefix + crypto.randomUUID();
  await persist(id, record);
  const page = await chrome.tabs.create({ url: chrome.runtime.getURL('save.html#' + id) });
  // The settings page manages the binding until it is closed. An active share continues on the source tab.
  await navigator.locks.request(id, async () => {
    const latest = await state(id);
    if (latest) await persist(id, { ...latest, controlTabId: page.id });
  });
});

async function handleMessage(id, op, automatic, controlTabId) {
  return navigator.locks.request(id, async () => {
    const record = await state(id);
    if (!record) throw Error('分享已结束，请在原用量页重新点击扩展。');
    if (record.controlTabId && controlTabId && record.controlTabId !== controlTabId) throw Error('请使用原分享窗口。');
    if (op === 'info') return record;
    if (record.error) throw Error(record.error);
    if (op === 'stop') {
      await stop(id, record, '定时更新已停用。');
    } else if (op === 'save') {
      if (automatic && !record.data.account) throw Error('无法确认页面账号。');
      try {
        // Serialize all output checks/writes, including two explicit selections of the same file.
        await navigator.locks.request('windao-quota-write', () => refresh(id, record, automatic));
        record.automatic = automatic;
        await persist(id, record);
        if (automatic) await chrome.alarms.create(id, { periodInMinutes: 5 });
        else await stop(id, record, record.note);
      } catch (error) { await stop(id, record, error.message); }
    }
    return record;
  });
}

chrome.runtime.onMessage.addListener((msg, sender, respond) => {
  const expected = chrome.runtime.getURL('save.html');
  if (sender.url?.split('#')[0] !== expected || typeof msg?.id !== 'string' || !msg.id.startsWith(prefix)) return;
  handleMessage(msg.id, msg.op, msg.automatic === true, sender.tab?.id)
    .then(respond).catch(error => respond({ error: error.message }));
  return true;
});

chrome.alarms.onAlarm.addListener(alarm => {
  if (!alarm.name.startsWith(prefix)) return;
  navigator.locks.request(alarm.name, async () => {
    const record = await state(alarm.name);
    if (!record?.automatic) { await chrome.alarms.clear(alarm.name); return; }
    try { await navigator.locks.request('windao-quota-write', () => refresh(alarm.name, record, true)); }
    catch (error) { await stop(alarm.name, record, error.message || '无法继续更新。'); }
  });
});

async function sourceChanged(tabId, closed) {
  const values = await session.get(null);
  await Promise.all(Object.entries(values).filter(([id]) => id.startsWith(prefix)).map(([id]) =>
    navigator.locks.request(id, async () => {
      const record = await state(id);
      if (!record) return;
      if (record.tabId === tabId) {
        await stop(id, record, closed ? '用量页已关闭，定时更新已停用。' : '用量页已重新导航，定时更新已停用。');
        if (closed && !record.controlTabId) await session.remove(id);
      } else if (closed && record.controlTabId === tabId) {
        if (!record.automatic) await session.remove(id);
        else { delete record.controlTabId; await persist(id, record); }
      }
    })
  ));
}

chrome.tabs.onRemoved.addListener(tabId => { sourceChanged(tabId, true); });
chrome.tabs.onUpdated.addListener((tabId, change) => {
  if (change.status === 'loading' || change.url) sourceChanged(tabId, false);
});
chrome.runtime.onStartup.addListener(() => fileRecord('', 'clear'));
