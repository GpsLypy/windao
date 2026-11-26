/* Runs on the selected usage document. Return percentages and a service-scoped account fingerprint. */
async function readQuota() {
  const { origin, pathname, search } = window.location;
  const gemini = origin === 'https://gemini.google.com' && /^\/(u\/\d+\/)?usage\/?$/.test(pathname);
  const cursor = origin === 'https://cursor.com' && (/^\/dashboard\/usage\/?$/.test(pathname) ||
    (/^\/dashboard\/?$/.test(pathname) && new URLSearchParams(search).get('tab') === 'usage'));
  if (!gemini && !cursor) return { error: '请在 Gemini 用量页或 Cursor 官方账户的 Grok Bot 用量页点击扩展。' };

  const visible = element => element.getClientRects().length > 0 &&
    (!element.checkVisibility || element.checkVisibility({ checkOpacity: true, checkVisibilityCSS: true }));
  const groups = gemini ? [
    ['GXU_FIVE_HOURLY', /(?:5|five)[ -]*(?:hours?|hr|小时)|五小时/i],
    ['GXU_WEEKLY', /weekly|this week|7[ -]*days?|每周|本周|一周|七天|7\s*天/i],
  ] : [['GrokBot', /grok\s*bot|\bsand\b/i]];
  const values = [];
  const records = [...document.querySelectorAll('section, article, li, [role="group"], div, p')];
  for (const [key, label] of groups) {
    const readings = new Set();
    for (const element of records) {
      if (!visible(element)) continue;
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
  const identities = new Set();
  for (const el of document.querySelectorAll('button, [role="button"], header a, nav a')) {
    if (!visible(el)) continue;
    const text = [el.getAttribute('aria-label'), el.getAttribute('title'), el.innerText].filter(Boolean).join(' ');
    if (text.length > 320 || !/account|profile|signed in|账户|帐号|账号|个人资料/i.test(text)) continue;
    for (const email of text.match(/[a-z0-9._%+-]+@[a-z0-9.-]+\.[a-z]{2,}/gi) || []) identities.add(email.toLowerCase());
  }
  let account = null;
  if (identities.size === 1) {
    const bytes = new TextEncoder().encode(origin + ':' + [...identities][0]);
    const hash = await crypto.subtle.digest('SHA-256', bytes);
    account = Array.from(new Uint8Array(hash), b => b.toString(16).padStart(2, '0')).join('');
  }
  return gemini ? {
    source: 'Gemini', account, report: { pools: values.map(([poolType, percent]) => ({ poolType, percentageUsed: percent / 100 })) }
  } : { source: 'GrokBot', account, report: { usagePercent: values[0][1] } };
}
