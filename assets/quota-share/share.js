/* Runs inside the selected tab on an explicit toolbar click. Only percentages leave the page. */
function readQuota() {
  const { origin, pathname } = window.location;
  const gemini = origin === 'https://gemini.google.com' && /^\/(u\/\d+\/)?usage\/?$/.test(pathname);
  const cursor = origin === 'https://cursor.com' && /^\/dashboard(?:\/|$)/.test(pathname);
  if (!gemini && !cursor) return { error: '请在 Gemini 用量页或 Cursor 官方账户的 Grok Bot 用量页点击扩展。' };

  const groups = gemini ? [
    ['GXU_FIVE_HOURLY', /(?:5|five)[ -]*(?:hours?|hr|小时)|五小时/i],
    ['GXU_WEEKLY', /weekly|this week|7[ -]*days?|每周|本周|一周|七天|7\s*天/i],
  ] : [['GrokBot', /grok\s*bot|\bsand\b/i]];
  const values = [];
  const records = [...document.querySelectorAll('section, article, li, [role="group"], div, p')];
  for (const [key, label] of groups) {
    const readings = new Set();
    for (const element of records) {
      if (!element.getClientRects().length) continue;
      const text = element.innerText?.trim();
      if (!text || text.length > 360 || !label.test(text)) continue;
      if (gemini && groups.some(([other, pattern]) => other !== key && pattern.test(text))) continue;
      if (cursor && !/weekly|this week|7[ -]*days?|每周|本周|一周|七天|7\s*天/i.test(text)) continue;
      const matches = [...text.matchAll(/(\d+(?:\.\d+)?)\s*[%％]/g)];
      if (matches.length !== 1) continue;
      const remaining = /remaining|available|\bleft\b|剩余|可用/i.test(text);
      const used = /\bused\b|consumed|已用|已使用|已消耗/i.test(text);
      if (remaining === used) continue;
      const percentage = Number(matches[0][1]);
      if (percentage > 100) continue;
      readings.add(remaining ? 100 - percentage : percentage);
    }
    if (readings.size === 1) values.push([key, [...readings][0]]);
  }
  if (!values.length) return { error: gemini ? '当前页没有可识别的用量窗口。请打开额度详情后重试。' : '当前页没有明确标为 Grok Bot 或 Sand 的已用或剩余额度。' };
  return gemini ? {
    source: 'Gemini', report: { pools: values.map(([poolType, percent]) => ({ poolType, percentageUsed: percent / 100 })) }
  } : { source: 'GrokBot', report: { usagePercent: values[0][1] } };
}

chrome.action.onClicked.addListener(async (tab) => {
  let result;
  try {
    const url = new URL(tab.url);
    if (!['https://gemini.google.com', 'https://cursor.com'].includes(url.origin)) throw Error();
    [{ result }] = await chrome.scripting.executeScript({ target: { tabId: tab.id }, func: readQuota });
  } catch {
    result = { error: '请在已登录的官方额度页面使用扩展。' };
  }
  const share = new URL(chrome.runtime.getURL('save.html'));
  share.hash = encodeURIComponent(JSON.stringify(result));
  await chrome.tabs.create({ url: share.href });
});
