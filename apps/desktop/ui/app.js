const invoke = window.__TAURI__.core.invoke;
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
async function updatePacks(selected) {
  const items = await invoke('packs');
  const previous = selected || packList.value;
  packList.replaceChildren();
  for (const item of items) {
    const option = document.createElement('option'); option.value = item.path; option.textContent = item.name; packList.append(option);
  }
  if (items.some(item => item.path === previous)) packList.value = previous;
}
document.querySelector('#import').addEventListener('click', async () => {
  const button = document.querySelector('#import'); button.disabled = true; message.textContent = '正在检查和复制角色包…';
  try { const selected = await invoke('import_pack', {path: document.querySelector('#pack-path').value.trim()}); await updatePacks(selected); message.textContent = '已导入，正在尝试切换角色…'; }
  catch (error) {message.textContent = String(error);}
  finally {button.disabled = false;}
});
document.querySelector('#switch').addEventListener('click', async () => {
  try { if (!packList.value) return; await invoke('select_pack',{path:packList.value}); message.textContent = '正在尝试切换角色…'; }
  catch (error) {message.textContent = String(error);}
});
const scale = document.querySelector('#scale');
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
perch.addEventListener('input', () => { perchValue.textContent = `${perch.value}%`; });
perch.addEventListener('change', async () => {
  try { await invoke('set_window_perch', {percent: Number(perch.value)}); }
  catch (error) { externalMessage.textContent = String(error); }
});
(async () => {
  try { await updatePacks(); const p = await invoke('preferences'); scale.value = p.scale; perch.value = p.window_perch; perchValue.textContent = `${p.window_perch}%`; showExternalPreference(p); document.querySelector('#scale-value').textContent = `${p.scale}%`; }
  catch (error) {message.textContent=String(error);}
})();
setInterval(async () => {
  try { const p = await invoke('preferences'); if (p.error) message.textContent = p.error; showExternalPreference(p); if (!perchEditing) { perch.value = p.window_perch; perchValue.textContent = `${p.window_perch}%`; } }
  catch (_) {}
},1000);
