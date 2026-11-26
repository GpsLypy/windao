const test = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const vm = require('node:vm');
const path = require('node:path');
const crypto = require('node:crypto').webcrypto;
const directory = path.join(__dirname, '../assets/quota-share');
const code = name => fs.readFileSync(path.join(directory, name), 'utf8');

const element = (text, visible = true) => ({
  innerText: text, getClientRects: () => visible ? [1] : [], getAttribute: () => '',
});
async function read(url, labels, accounts) {
  const ctx = vm.createContext({ window: { location: new URL(url) }, URLSearchParams, crypto, TextEncoder,
    document: { querySelectorAll: selector => selector.startsWith('button') ? accounts.map(v => element(v)) : labels.map(v => element(v)) } });
  return vm.runInContext(code('quota.js') + '\nreadQuota()', ctx);
}
const gemini = 'https://gemini.google.com/u/0/usage';
const cursor = 'https://cursor.com/dashboard?tab=usage';
test('reject matching text outside exact official usage routes and unrelated Cursor totals', async () => {
  for (const url of ['https://gemini.google.com/app', 'https://cursor.com/dashboard', 'https://cursor.com/dashboard/team',
    'https://other.invalid/dashboard/usage','https://gemini.google.com.evil.invalid/usage']) {
    assert.ok((await read(url,['Grok Bot weekly 37% used'],[])).error);
  }
  assert.ok((await read(cursor,['Cursor weekly 33% used'],[])).error);
  assert.equal((await read(cursor,['Grok Bot weekly 70% remaining'],[])).report.usagePercent,30);
});

test('only reports clear visible percentages and unique account hash', async () => {
  const alice = await read(gemini,['5 hours 42% used','weekly 15% remaining'],['Google Account a@example.invalid']);
  const bob = await read(gemini,['weekly 0% used'],['Google Account b@example.invalid']);
  assert.equal(alice.report.pools[0].percentageUsed,0.42);
  assert.equal(alice.report.pools[1].percentageUsed,0.85);
  assert.match(alice.account,/^[0-9a-f]{64}$/);
  assert.notEqual(alice.account,bob.account);
  assert.equal(JSON.stringify(alice).includes('example.invalid'),false);
  const ambiguous = await read(gemini,['weekly 20% used'],['Account a@example.invalid','Account b@example.invalid']);
  assert.equal(ambiguous.account,null);
  assert.ok((await read(gemini,['weekly 12% used','weekly 22% used'],[])).error);
});

function file(name) {
  return { name, permission:'granted', value:'', async queryPermission(){ return this.permission; },
    async getFile(){ return {size:this.value.length, text:async()=>this.value}; },
    async createWritable(){ let next='';return {write:async text=>{next=text;},close:async()=>{this.value=next;},abort:async()=>{}}; } };
}
async function harness() {
  const states={}, handles=new Map(), alarms=new Set(), callbacks={};
  const hook = key => ({addListener: fn => { callbacks[key]=fn; }});
  const session = { get: async key => structuredClone(key === null ? states : {[key]:states[key]}),
    set: async value=>Object.assign(states,structuredClone(value)), remove:async key=>{delete states[key];} };
  let frame;
  const ctx=vm.createContext({ console, URL, importScripts:()=>{}, readQuota(){}, navigator:{locks:{request:async (_,op)=>op()}},
    chrome:{storage:{session}, action:{onClicked:hook('click')}, scripting:{executeScript:async ({target})=>{
      assert.deepEqual([...target.documentIds],['original']); return [{result:frame,documentId:'original'}];
    }},runtime:{onStartup:hook('startup'),onMessage:hook('message')}, tabs:{onRemoved:hook('close'),onUpdated:hook('navigate')},
    alarms:{onAlarm:hook('tick'),create:async(id,settings)=>{assert.equal(settings.periodInMinutes,5);alarms.add(id)},clear:async id=>alarms.delete(id)}}});
  vm.runInContext(code('files.js')+'\n'+code('share.js'),ctx);
  ctx.fileRecord=async (id,action)=>action==='get'?handles.get(id):handles.delete(id);
  async function bind(id,account,dest,tabId=7){
    frame={source:'Gemini',account,report:{pools:[{poolType:'GXU_WEEKLY',percentageUsed:0.23}]}};
    states[id]={tabId,documentId:'original',data:structuredClone(frame),automatic:false,hasSaved:false};
    handles.set(id,dest);
    return ctx.handleMessage(id,'save',true);
  }
  return {ctx,states,handles,alarms,bind, changeUsage: value=>{frame.report.pools[0].percentageUsed=value;}, changeAccount: account=>{frame={...frame,account};},
    async tick(name){await callbacks.tick({name});await new Promise(resolve=>setImmediate(resolve));} };
}

test('separate same-service accounts cannot overwrite each other and read/write revocation stops the alarm',async()=>{
  const h=await harness();const a=file('personal.json'),b=file('work.json');
  await h.bind('quota-a','A',a);await h.bind('quota-b','B',b,9);
  assert.equal(JSON.parse(a.value).windaoShare.account,'A');assert.equal(JSON.parse(b.value).windaoShare.account,'B');
  const earlier=a.value;
  await h.bind('quota-wrong','B',a,8);
  assert.equal(a.value,earlier);assert.equal(h.states['quota-wrong'].automatic,false);
  a.permission='denied';h.changeAccount('A');await h.tick('quota-a');
  assert.equal(a.value,earlier);assert.equal(h.alarms.has('quota-a'),false);
  h.changeAccount('C');await h.tick('quota-b');
  assert.equal(h.states['quota-b'].automatic,false);assert.equal(JSON.parse(b.value).windaoShare.account,'B');
});

test('manual stop, uncertain account, original page navigation and closure revoke binding',async()=>{
  for (const stop of ['manual','unknown','navigate','close']) {
    const h=await harness();const dest=file('test.json');await h.bind('quota-one','A',dest);
    if(stop==='manual') await h.ctx.handleMessage('quota-one','stop');
    else if(stop==='unknown'){h.changeAccount(null);await h.tick('quota-one');}
    else await h.ctx.sourceChanged(7,stop==='close');
    assert.equal(h.alarms.has('quota-one'),false);assert.equal(h.handles.has('quota-one'),false);
  }
});

test('each scheduled tick reads the pinned original document and updates the selected file', async()=>{
  const h=await harness(); const dest=file('mine.json');
  await h.bind('quota-on','A',dest);
  h.changeUsage(0.71); await h.tick('quota-on');
  assert.equal(JSON.parse(dest.value).pools[0].percentageUsed,0.71);
  assert.equal(h.states['quota-on'].automatic,true);
  assert.equal(h.alarms.has('quota-on'),true);
});
