const invoke = window.__TAURI__.core.invoke;
const careFields = ['satiety', 'energy', 'mood', 'intimacy'];
const careMessage = document.querySelector('#care-message');
function showCare(state) {
  for (const field of careFields) document.querySelector(`#care-${field}`).textContent = `${state.needs[field]}/100`;
  document.querySelector('#care-food').textContent = state.food;
}
async function refreshCare() {
  try { showCare(await invoke('care_status')); }
  catch (error) { careMessage.textContent = String(error); }
}
document.querySelectorAll('[data-care]').forEach(button => button.addEventListener('click', async () => {
  button.disabled = true;
  const action = button.dataset.care;
  const requestId = crypto.randomUUID();
  try {
    const result = await invoke('care_action', { requestId, action });
    showCare(result.state);
    careMessage.textContent = {feed: '已喂食', play: '玩耍完成', rest: '已休息'}[action];
  } catch (error) {
    careMessage.textContent = String(error);
    await refreshCare();
  } finally { button.disabled = false; }
}));
refreshCare();
setInterval(refreshCare, 60_000);
const companionEnabled = document.querySelector('#companion-enabled');
const companionDnd = document.querySelector('#companion-dnd');
const screenPlayEnabled = document.querySelector('#screen-play-enabled');
const companionInterval = document.querySelector('#companion-interval');
const companionLimit = document.querySelector('#companion-limit');
const companionMessage = document.querySelector('#companion-message');
async function saveCompanion() {
  const settings = {
    enabled: companionEnabled.checked,
    do_not_disturb: companionDnd.checked,
    screen_play_enabled: screenPlayEnabled.checked,
    interval_minutes: Number(companionInterval.value),
    hourly_limit: Number(companionLimit.value),
  };
  try { await invoke('set_companion_settings', { settings }); companionMessage.textContent = '设置已保存'; }
  catch (error) { companionMessage.textContent = String(error); }
}
[companionEnabled, companionDnd, screenPlayEnabled, companionInterval, companionLimit].forEach(input => input.addEventListener('change', saveCompanion));
document.querySelector('#stop-activity').addEventListener('click', async () => {
  try { await invoke('stop_activity'); companionMessage.textContent = '已请求停止当前互动'; }
  catch (error) { companionMessage.textContent = String(error); }
});
const activityNames = {Idle:'待机', Dragged:'拖拽中', Perched:'吸附中', Eating:'进食', Playing:'玩耍', Sleeping:'休息', Greeting:'招呼', Peeking:'探头', Inviting:'邀请', ScreenPlay:'占屏中'};
async function refreshCompanion() {
  try {
    const activity = await invoke('companion_activity');
    document.querySelector('#companion-activity').textContent = `当前：${activityNames[activity] || activity}`;
    const settings = await invoke('companion_settings');
    if (document.activeElement !== companionEnabled) companionEnabled.checked = settings.enabled;
    if (document.activeElement !== companionDnd) companionDnd.checked = settings.do_not_disturb;
    if (document.activeElement !== screenPlayEnabled) screenPlayEnabled.checked = settings.screen_play_enabled;
  } catch (error) { companionMessage.textContent = String(error); }
}
(async () => {
  try {
    const settings = await invoke('companion_settings');
    companionEnabled.checked = settings.enabled; companionDnd.checked = settings.do_not_disturb; screenPlayEnabled.checked = settings.screen_play_enabled;
    companionInterval.value = settings.interval_minutes; companionLimit.value = settings.hourly_limit;
  } catch (error) { companionMessage.textContent = String(error); }
})();
refreshCompanion();
setInterval(refreshCompanion, 1000);
const labels = { starting: '启动中', connecting: '连接中', ready: '已连接', recovering: '正在恢复', fault: '需要处理' };
async function refresh() {
  try {
    const s = await invoke('status');
    document.querySelector('#status').textContent = `${labels[s.phase] || s.phase} · ${s.visible ? '请求显示' : '请求隐藏'}`;
    document.querySelector('#detail').textContent = s.detail;
  } catch (error) { document.querySelector('#detail').textContent = String(error); }
}
document.querySelectorAll('[data-action]').forEach(button => button.addEventListener('click', async () => {
  try { await invoke('control', { action: button.dataset.action }); await refresh(); }
  catch (error) { document.querySelector('#detail').textContent = String(error); }
}));
refresh();
setInterval(refresh, 1000);
const packList = document.querySelector('#pack-list');
const message = document.querySelector('#import-message');
const activePack = document.querySelector('#active-pack');
let lastTouchSeen = null;
async function refreshTouchCare() {
  const view = await invoke('last_touch');
  if (view) {
    const touchKey = `${view.outcome.occurred_utc_ms}:${view.event_id}`;
    if (touchKey !== lastTouchSeen) { showCare(view.outcome.state); lastTouchSeen = touchKey; }
  }
}
setInterval(() => { refreshTouchCare().catch(() => {}); }, 300);
let pendingSelectionId = null;
function packName(path) {
  return [...packList.options].find(option => option.value === path)?.textContent || path?.split('/').slice(-2, -1)[0] || '未选择';
}
function showSelection(preferences) {
  const active = preferences.active_pack;
  activePack.textContent = `当前角色：${packName(active)}`;
  const selection = preferences.selection_status;
  if (pendingSelectionId === null || selection?.id !== pendingSelectionId) return;
  if (selection.phase === 'succeeded') {
    pendingSelectionId = null;
    if (active) packList.value = active;
    message.textContent = `已切换到 ${packName(active)}`;
    lastTouchSeen = null;
  } else if (selection.phase === 'failed') {
    pendingSelectionId = null;
    message.textContent = `${selection.error || '切换失败'}；当前仍为 ${packName(active)}`;
  }
}
async function updatePacks(selected) {
  const items = await invoke('packs');
  const previous = selected || packList.value;
  packList.replaceChildren();
  for (const item of items) {
    const option = document.createElement('option'); option.value = item.path; option.textContent = item.name; packList.append(option);
  }
  if (items.some(item => item.path === previous)) packList.value = previous;
  return items;
}
document.querySelector('#import').addEventListener('click', async () => {
  const button = document.querySelector('#import'); button.disabled = true; message.textContent = '正在检查和复制角色包…';
  try { const selected = await invoke('import_pack', {path: document.querySelector('#pack-path').value.trim()}); pendingSelectionId = selected.id; await updatePacks(selected.path); message.textContent = '已导入，正在尝试切换角色…'; showSelection(await invoke('preferences')); }
  catch (error) {message.textContent = String(error);}
  finally {button.disabled = false;}
});
document.querySelector('#switch').addEventListener('click', async () => {
  try { if (!packList.value) return; const selected = await invoke('select_pack',{path:packList.value}); pendingSelectionId = selected.id; message.textContent = '正在尝试切换角色…'; showSelection(await invoke('preferences')); }
  catch (error) {message.textContent = String(error);}
});
const scale = document.querySelector('#scale');
const gazeRadius = document.querySelector('#gaze-radius');
const gazeRadiusValue = document.querySelector('#gaze-radius-value');
let gazeEditing = false;
gazeRadius.addEventListener('pointerdown', () => { gazeEditing = true; });
gazeRadius.addEventListener('pointerup', () => { gazeEditing = false; });
gazeRadius.addEventListener('pointercancel', () => { gazeEditing = false; });
gazeRadius.addEventListener('keydown', () => { gazeEditing = true; });
gazeRadius.addEventListener('keyup', () => { gazeEditing = false; });
const externalSnap = document.querySelector('#external-snap');
const externalMessage = document.querySelector('#external-message');
const perch = document.querySelector('#window-perch');
const perchValue = document.querySelector('#window-perch-value');
let perchEditing = false;
perch.addEventListener('pointerdown', () => { perchEditing = true; });
perch.addEventListener('pointerup', () => { perchEditing = false; });
perch.addEventListener('pointercancel', () => { perchEditing = false; });
perch.addEventListener('keydown', () => { perchEditing = true; });
perch.addEventListener('keyup', () => { perchEditing = false; });
const externalHint = externalMessage.textContent;
function showExternalPreference(p) {
  externalSnap.checked = p.external_snap;
  externalMessage.textContent = p.external_error || (p.external_snap ? '他应用窗口吸附已开启。' : externalHint);
}
externalSnap.addEventListener('change', async () => {
  try { await invoke('set_external_snap', {enabled: externalSnap.checked}); }
  catch (error) { externalMessage.textContent = String(error); externalSnap.checked = false; }
});
scale.addEventListener('input', () => document.querySelector('#scale-value').textContent = `${scale.value}%`);
scale.addEventListener('change', async () => {
  try {await invoke('set_scale',{scale:Number(scale.value)});} catch (error) {message.textContent = String(error);}
});
gazeRadius.addEventListener('input', () => { gazeRadiusValue.textContent = `${gazeRadius.value} 点`; });
gazeRadius.addEventListener('change', async () => {
  try { await invoke('set_gaze_radius', {radius: Number(gazeRadius.value)}); }
  catch (error) { message.textContent = String(error); }
});
perch.addEventListener('input', () => { perchValue.textContent = `${perch.value}%`; });
perch.addEventListener('change', async () => {
  try { await invoke('set_window_perch', {percent: Number(perch.value)}); }
  catch (error) { externalMessage.textContent = String(error); }
});
(async () => {
  try { const p = await invoke('preferences'); await updatePacks(p.active_pack); showSelection(p); scale.value = p.scale; gazeRadius.value = p.gaze_radius; gazeRadiusValue.textContent = `${p.gaze_radius} 点`; perch.value = p.window_perch; perchValue.textContent = `${p.window_perch}%`; showExternalPreference(p); document.querySelector('#scale-value').textContent = `${p.scale}%`; }
  catch (error) {message.textContent=String(error);}
})();
setInterval(async () => {
  try { const p = await invoke('preferences'); showSelection(p); if (p.error && pendingSelectionId === null && !p.selection_status) message.textContent = p.error; showExternalPreference(p); if (!gazeEditing) { gazeRadius.value = p.gaze_radius; gazeRadiusValue.textContent = `${p.gaze_radius} 点`; } if (!perchEditing) { perch.value = p.window_perch; perchValue.textContent = `${p.window_perch}%`; } }
  catch (_) {}
},1000);
