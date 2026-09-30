const invoke = window.__TAURI__.core.invoke;
const languageSelect = document.querySelector('#ui-language');
languageSelect.addEventListener('change', async () => {
  try {
    await invoke('set_ui_language', {language: languageSelect.value});
    applyLanguage(languageSelect.value);
    renderVoiceProviders();
    refreshVoiceStatus();
    refreshMemory(true);
    refresh();
    refreshCompanion();
    invoke('preferences').then(showSelection).catch(() => {});
  } catch (error) { languageSelect.value = currentLanguage; document.querySelector('#detail').textContent = String(error); }
});
invoke('ui_language').then(language => {
  languageSelect.value = language;
  applyLanguage(language);
}).catch(error => { document.querySelector('#detail').textContent = String(error); });
const voiceMessage = document.querySelector('#voice-message');
const voicePhases = {idle:'待机', starting:'准备中', listening:'聆听中', recognizing:'识别中', thinking:'生成回复', searching:'联网查询中', synthesizing:'合成语音', speaking:'播放中', faulted:'需要处理'};
const voiceFields = {
  enabled:'voice-enabled', asr_backend:'voice-asr-backend', llm_backend:'voice-llm-backend', tts_backend:'voice-tts-backend',
  model_dir:'voice-model-dir', asr_model_path:'voice-asr-model-path', asr_threads:'voice-asr-threads', asr_gpu:'voice-asr-gpu',
  asr_ncnn_model_dir:'voice-asr-ncnn-model-dir', asr_ncnn_executable:'voice-asr-ncnn-executable', asr_ncnn_threads:'voice-asr-ncnn-threads', asr_ncnn_gpu:'voice-asr-ncnn-gpu',
  tts_model_dir:'voice-tts-model-dir', tts_threads:'voice-tts-threads', tts_gpu:'voice-tts-gpu', tts_speaker:'voice-tts-speaker', tts_service_url:'voice-tts-service-url',
  kokoro_model_dir:'voice-kokoro-model-dir', kokoro_threads:'voice-kokoro-threads', kokoro_gpu:'voice-kokoro-gpu',
  vad_model_path:'voice-vad-model-path', vad_threshold:'voice-vad-threshold', vad_silence_ms:'voice-vad-silence', no_speech_timeout_secs:'voice-no-speech-timeout', rest_after_inactive_minutes:'voice-rest-inactive',
  aec_enabled:'voice-aec-enabled', kws_enabled:'voice-kws-enabled', kws_model_dir:'voice-kws-model-dir', kws_keyword:'voice-kws-keyword', kws_keyword_en:'voice-kws-keyword-en', kws_keywords_file:'voice-kws-keywords-file', kws_threshold:'voice-kws-threshold', kws_threads:'voice-kws-threads',
  wake_greeting_enabled:'voice-wake-greeting', timed_greetings_enabled:'voice-timed-greetings', first_greeting_enabled:'voice-first-greeting', greeting_dir:'voice-greeting-dir', birthday:'voice-birthday',
  reference_audio:'voice-reference-audio', reference_text:'voice-reference-text', lm_studio_url:'voice-lm-url', lm_studio_model:'voice-lm-model', llm_api_key:'voice-llm-api-key', system_prompt:'voice-system-prompt', remember_context:'voice-remember-context', output_volume_percent:'voice-output-volume', web_enabled:'voice-web-enabled', search_provider:'voice-search-provider', weather_city:'voice-weather-city'
};
function voiceInput(key) { return document.getElementById(voiceFields[key]); }
function renderVoiceProviders() {
  const asr = voiceInput('asr_backend').value;
  const tts = voiceInput('tts_backend').value;
  const llm = voiceInput('llm_backend').value;
  for (const panel of document.querySelectorAll('[data-provider-for]')) {
    const [kind, value] = panel.dataset.providerFor.split(':');
    panel.hidden = (kind === 'asr' ? asr : tts) !== value;
  }
  const planned = tts !== 'sherpa_onnx' || ['anthropic','gemini'].includes(llm);
  document.querySelector('#voice-provider-note').textContent = planned ? t('所选后端处于规划阶段，当前可以查看和填写参数；启用语音前仍需切回已接入的后端。') : '';
  document.querySelector('#voice-start').disabled = planned || !voiceInput('enabled').checked;
}
for (const key of ['asr_backend','tts_backend','llm_backend','enabled']) voiceInput(key).addEventListener('change', renderVoiceProviders);
voiceInput('output_volume_percent').addEventListener('input', () => {
  document.querySelector('#voice-output-volume-value').textContent = `${voiceInput('output_volume_percent').value}%`;
});
const voiceBackendUrls = {lm_studio:'http://127.0.0.1:1234', ollama:'http://127.0.0.1:11434/v1', llama_cpp:'http://127.0.0.1:8080/v1', open_ai:'https://api.openai.com/v1'};
voiceInput('llm_backend').addEventListener('change', () => {
  const url = voiceInput('lm_studio_url');
  if (!url.value.trim() || Object.values(voiceBackendUrls).includes(url.value.trim())) {
    url.value = voiceBackendUrls[voiceInput('llm_backend').value] || url.value;
  }
});
async function refreshVoiceSettings() {
  try {
    const settings = await invoke('voice_settings');
    for (const [key, id] of Object.entries(voiceFields)) {
      const input = document.getElementById(id);
      if (input.type === 'checkbox') input.checked = Boolean(settings[key]);
      else input.value = key === 'asr_backend' && settings[key] === 'sherpa_mlx' ? 'sherpa_onnx' : settings[key] ?? '';
    }
    renderVoiceProviders();
    document.querySelector('#voice-output-volume-value').textContent = `${settings.output_volume_percent}%`;
  } catch (error) { voiceMessage.textContent = String(error); }
}
async function saveVoiceSettings() {
  const settings = {};
  for (const key of Object.keys(voiceFields)) {
    const input = voiceInput(key);
    settings[key] = input.type === 'checkbox' ? input.checked : ['number','range'].includes(input.type) ? Number(input.value) : input.value.trim();
  }
  try { await invoke('set_voice_settings', {settings}); voiceMessage.textContent = t('语音设置已保存；点击角色头部开始对话。'); }
  catch (error) { voiceMessage.textContent = String(error); throw error; }
}
document.querySelector('#voice-save').addEventListener('click', () => { saveVoiceSettings().catch(() => {}); });
voiceInput('kws_enabled').addEventListener('change', () => {
  saveVoiceSettings().catch(() => { voiceInput('kws_enabled').checked = !voiceInput('kws_enabled').checked; });
});
document.querySelector('#voice-start').addEventListener('click', () => invoke('voice_start').catch(error => voiceMessage.textContent = String(error)));
document.querySelector('#voice-stop').addEventListener('click', () => invoke('voice_stop').catch(error => voiceMessage.textContent = String(error)));
const webSettingsStatus = document.querySelector('#web-settings-status');
let webSaveQueue = Promise.resolve();
function saveWebSettings() {
  const values = {
    webEnabled:voiceInput('web_enabled').checked,
    searchProvider:voiceInput('search_provider').value,
    weatherCity:voiceInput('weather_city').value.trim()
  };
  webSaveQueue = webSaveQueue.catch(() => {}).then(async () => {
    await invoke('set_web_settings', values);
    webSettingsStatus.textContent = t('联网设置已自动保存');
  });
  return webSaveQueue.catch(error => { webSettingsStatus.textContent = String(error); throw error; });
}
for (const key of ['web_enabled','search_provider','weather_city']) {
  voiceInput(key).addEventListener('change', () => { saveWebSettings().catch(() => {}); });
}
async function refreshBraveKeyStatus() {
  try { document.querySelector('#brave-key-status').textContent = t(await invoke('brave_key_status') ? '密钥已保存在钥匙串' : '未保存密钥'); }
  catch (error) { document.querySelector('#brave-key-status').textContent = String(error); }
}
document.querySelector('#brave-key-save').addEventListener('click', async () => {
  const input = document.querySelector('#brave-key');
  try { await invoke('brave_key_save', {key:input.value.trim()}); input.value = ''; await refreshBraveKeyStatus(); }
  catch (error) { document.querySelector('#brave-key-status').textContent = String(error); }
});
document.querySelector('#brave-key-clear').addEventListener('click', async () => {
  try { await invoke('brave_key_clear'); await refreshBraveKeyStatus(); }
  catch (error) { document.querySelector('#brave-key-status').textContent = String(error); }
});
refreshBraveKeyStatus();
function showWebSources(container, sources) {
  container.replaceChildren();
  for (const source of sources || []) {
    const button = document.createElement('button'); button.type = 'button'; button.className = 'source-link';
    const checked = source.retrieved_at ? new Date(source.retrieved_at * 1000).toLocaleString() : '';
    button.textContent = `${source.id} · ${source.title || source.url}${checked ? ` · ${checked}` : ''}`;
    button.title = source.url;
    button.addEventListener('click', () => invoke('open_web_source', {url:source.url}).catch(error => document.querySelector('#agent-status').textContent = String(error)));
    container.append(button);
  }
}
let agentRequest = 0;
document.querySelector('#agent-query').addEventListener('click', async () => {
  const question = document.querySelector('#agent-query-input').value.trim();
  if (!question) { document.querySelector('#agent-status').textContent = t('请输入问题'); return; }
  const request = ++agentRequest;
  const status = document.querySelector('#agent-status'); status.textContent = t('正在联网查询…');
  document.querySelector('#agent-answer').textContent = '';
  showWebSources(document.querySelector('#agent-sources'), []);
  try {
    await saveWebSettings();
    if (request !== agentRequest) return;
    const answer = await invoke('agent_query', {question});
    if (request !== agentRequest) return;
    document.querySelector('#agent-answer').textContent = answer.answer_text;
    showWebSources(document.querySelector('#agent-sources'), answer.sources);
    status.textContent = `${t('查询完成')} · ${answer.sources.length} ${t('个来源')}`;
  } catch (error) { if (request === agentRequest) status.textContent = String(error); }
});
document.querySelector('#agent-cancel').addEventListener('click', async () => {
  agentRequest++; document.querySelector('#agent-status').textContent = t('查询已停止');
  try { await invoke('agent_cancel'); } catch (error) { document.querySelector('#agent-status').textContent = String(error); }
});
refreshVoiceSettings();
const memoryEnabled = document.querySelector('#memory-enabled');
const memoryStatus = document.querySelector('#memory-status');
const memoryItems = document.querySelector('#memory-items');
const memorySearch = document.querySelector('#memory-search');
const memoryListStatus = document.querySelector('#memory-list-status');
const memoryClearConfirm = document.querySelector('#memory-clear-confirm');
let memorySnapshot = null;
let memoryRefreshSequence = 0;
function memoryButton(label, handler, className = '') {
  const button = document.createElement('button');
  button.type = 'button'; button.textContent = t(label); button.className = className;
  button.addEventListener('click', handler);
  return button;
}
function renderMemoryItems(resetScroll = false) {
  if (!memorySnapshot) return;
  const scrollTop = resetScroll ? 0 : memoryItems.scrollTop;
  const query = memorySearch.value.trim().toLocaleLowerCase();
  const matches = memorySnapshot.items.filter(item => item.content.toLocaleLowerCase().includes(query));
  document.querySelector('#memory-count').textContent = `${t('显示')} ${matches.length} / ${memorySnapshot.items.length} ${t('条记忆')}`;
  memoryItems.replaceChildren();
  if (!matches.length) {
    const empty = document.createElement('p'); empty.className = 'hint';
    empty.textContent = t(query ? '没有匹配的记忆' : '还没有记忆');
    memoryItems.append(empty);
  }
  for (const item of matches) {
    const row = document.createElement('div'); row.className = 'memory-item';
    const badge = document.createElement('span'); badge.className = 'memory-badge'; badge.textContent = t(item.confirmed ? '已确认' : '待确认');
    const input = document.createElement('textarea'); input.rows = 2; input.maxLength = 500; input.value = item.content; input.setAttribute('aria-label', t('记忆内容'));
    const controls = document.createElement('div'); controls.className = 'memory-controls';
    const confirmed = document.createElement('input'); confirmed.type = 'checkbox'; confirmed.checked = item.confirmed;
    const confirmLabel = document.createElement('label'); confirmLabel.className = 'check'; confirmLabel.append(confirmed, document.createTextNode(t('确认可用于回复')));
    const pinned = document.createElement('input'); pinned.type = 'checkbox'; pinned.checked = item.pinned;
    const pinLabel = document.createElement('label'); pinLabel.className = 'check'; pinLabel.append(pinned, document.createTextNode(t('固定')));
    controls.append(confirmLabel, pinLabel);
    controls.append(memoryButton('保存', async () => {
      try {
        await invoke('memory_update', {id:item.id, content:input.value, confirmed:confirmed.checked, pinned:pinned.checked});
        memoryListStatus.textContent = t('记忆已保存'); await refreshMemory(true);
      } catch (error) { memoryListStatus.textContent = String(error); }
    }));
    const forget = memoryButton('忘记', () => { memoryRefreshSequence++; forget.hidden = true; confirmation.hidden = false; }, 'danger');
    controls.append(forget);
    const confirmation = document.createElement('div'); confirmation.className = 'memory-confirm'; confirmation.hidden = true;
    const warning = document.createElement('span'); warning.textContent = t('忘记这条记忆，并清除对话摘要和未压缩记录？');
    const accept = memoryButton('确认忘记', async () => {
      accept.disabled = true;
      try {
        await invoke('memory_forget', {id:item.id});
        memoryListStatus.textContent = t('记忆已删除'); await refreshMemory(true);
      } catch (error) { accept.disabled = false; memoryListStatus.textContent = String(error); }
    }, 'danger');
    const cancel = memoryButton('取消', () => { confirmation.hidden = true; forget.hidden = false; });
    confirmation.append(warning, accept, cancel);
    row.append(badge, input, controls, confirmation); memoryItems.append(row);
  }
  memoryItems.scrollTop = scrollTop;
}
async function refreshMemory(force = false) {
  if (!force && (document.activeElement?.closest('#memory-items') || document.activeElement === memorySearch || memoryItems.querySelector('.memory-confirm:not([hidden])') || !memoryClearConfirm.hidden)) return;
  const sequence = ++memoryRefreshSequence;
  try {
    const snapshot = await invoke('memory_snapshot');
    if (sequence !== memoryRefreshSequence) return;
    memorySnapshot = snapshot;
    memoryEnabled.checked = snapshot.enabled;
    document.querySelector('#memory-progress').textContent = `${t('待整理对话')}: ${snapshot.uncompressed_turns}`;
    const summaryInput = document.querySelector('#memory-summary');
    if (document.activeElement !== summaryInput) summaryInput.value = snapshot.summary;
    renderMemoryItems();
  } catch (error) { if (sequence === memoryRefreshSequence) memoryStatus.textContent = String(error); }
}
memorySearch.addEventListener('input', () => renderMemoryItems(true));
memoryEnabled.addEventListener('change', async () => {
  try { await invoke('memory_set_enabled', {enabled:memoryEnabled.checked}); memoryStatus.textContent = t(memoryEnabled.checked ? '长期记忆已开启' : '长期记忆已关闭，已有记录仍保留'); await refreshMemory(); }
  catch (error) { memoryEnabled.checked = !memoryEnabled.checked; memoryStatus.textContent = String(error); }
});
document.querySelector('#memory-add').addEventListener('click', async () => {
  const input = document.querySelector('#memory-new');
  try { await invoke('memory_add', {content:input.value}); input.value = ''; memoryListStatus.textContent = t('记忆已保存'); await refreshMemory(true); }
  catch (error) { memoryListStatus.textContent = String(error); }
});
document.querySelector('#memory-clear').addEventListener('click', () => { memoryClearConfirm.hidden = false; });
document.querySelector('#memory-clear-no').addEventListener('click', () => { memoryClearConfirm.hidden = true; });
document.querySelector('#memory-clear-yes').addEventListener('click', async event => {
  event.currentTarget.disabled = true;
  try {
    await invoke('memory_clear'); memoryClearConfirm.hidden = true; memorySearch.value = '';
    document.querySelector('#memory-clear-status').textContent = t('全部记忆已清空'); await refreshMemory(true);
  } catch (error) { document.querySelector('#memory-clear-status').textContent = String(error); }
  finally { event.currentTarget.disabled = false; }
});
document.querySelector('#memory-summary-save').addEventListener('click', async () => {
  try { await invoke('memory_set_summary', {summary:document.querySelector('#memory-summary').value}); document.querySelector('#memory-summary-status').textContent = t('摘要已保存'); await refreshMemory(true); }
  catch (error) { document.querySelector('#memory-summary-status').textContent = String(error); }
});
document.querySelector('#memory-summary-clear').addEventListener('click', async () => {
  const summary = document.querySelector('#memory-summary');
  if (!summary.value.trim()) { document.querySelector('#memory-summary-status').textContent = t('摘要已经为空'); return; }
  try { await invoke('memory_set_summary', {summary:''}); summary.value = ''; document.querySelector('#memory-summary-status').textContent = t('摘要已清空'); await refreshMemory(true); }
  catch (error) { document.querySelector('#memory-summary-status').textContent = String(error); }
});
refreshMemory();
setInterval(refreshMemory, 5000);
async function refreshVoiceStatus() {
  try {
    const status = await invoke('voice_status');
    document.querySelector('#voice-state').textContent = `${t('语音')}: ${t(voicePhases[status.phase] || status.phase || '待机')}${status.detail ? ` · ${t(status.detail)}` : ''}`;
    document.querySelector('#voice-transcript').textContent = status.transcript ? `${{'zh-CN':'你说','en-US':'You said','ja-JP':'あなた','ko-KR':'사용자'}[currentLanguage]}: ${status.transcript}` : '';
    document.querySelector('#voice-response').textContent = status.response ? `${{'zh-CN':'回复','en-US':'Reply','ja-JP':'返答','ko-KR':'응답'}[currentLanguage]}: ${status.response}` : '';
    const sourceKey = JSON.stringify(status.sources || []);
    if (sourceKey !== refreshVoiceStatus.lastSources) {
      showWebSources(document.querySelector('#voice-sources'), status.sources);
      refreshVoiceStatus.lastSources = sourceKey;
    }
    const kws = status.kws_status || 'off';
    const kwsLabel = kws.startsWith('error:') ? `${t('需要处理')}: ${kws.slice(6)}` :
      kws.startsWith('matched:') ? `${t('已唤醒')}: ${kws.slice(8)}` :
      t({off:'已关闭', voice_off:'请先启用语音交互', loading:'加载模型中', listening:'正在监听', paused:'对话期间暂停'}[kws] || kws);
    document.querySelector('#voice-kws-state').textContent = `KWS: ${kwsLabel}`;
  } catch (_) {}
}
refreshVoiceStatus();
setInterval(refreshVoiceStatus, 400);
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
    careMessage.textContent = t({feed: '已喂食', play: '玩耍完成', rest: '已休息'}[action]);
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
  try { await invoke('set_companion_settings', { settings }); companionMessage.textContent = t('设置已保存'); }
  catch (error) { companionMessage.textContent = String(error); }
}
[companionEnabled, companionDnd, screenPlayEnabled, companionInterval, companionLimit].forEach(input => input.addEventListener('change', saveCompanion));
document.querySelector('#stop-activity').addEventListener('click', async () => {
  try { await invoke('stop_activity'); companionMessage.textContent = t('已请求停止当前互动'); }
  catch (error) { companionMessage.textContent = String(error); }
});
const activityNames = {Idle:'待机', Dragged:'拖拽中', Perched:'吸附中', Eating:'进食', Playing:'玩耍', Sleeping:'休息', Greeting:'招呼', Peeking:'探头', Inviting:'邀请', ScreenPlay:'占屏中'};
async function refreshCompanion() {
  try {
    const activity = await invoke('companion_activity');
    document.querySelector('#companion-activity').textContent = `${{'zh-CN':'当前','en-US':'Current','ja-JP':'現在','ko-KR':'현재'}[currentLanguage]}: ${t(activityNames[activity] || activity)}`;
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
    document.querySelector('#status').textContent = `${t(labels[s.phase] || s.phase)} · ${t(s.visible ? '请求显示' : '请求隐藏')}`;
    document.querySelector('#detail').textContent = t(s.detail);
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
  return [...packList.options].find(option => option.value === path)?.textContent || path?.split('/').slice(-2, -1)[0] || t('未选择');
}
function showSelection(preferences) {
  const active = preferences.active_pack;
  activePack.textContent = `${{'zh-CN':'当前角色','en-US':'Current character','ja-JP':'現在のキャラクター','ko-KR':'현재 캐릭터'}[currentLanguage]}: ${packName(active)}`;
  const selection = preferences.selection_status;
  if (pendingSelectionId === null || selection?.id !== pendingSelectionId) return;
  if (selection.phase === 'succeeded') {
    pendingSelectionId = null;
    if (active) packList.value = active;
    message.textContent = `${{'zh-CN':'已切换到','en-US':'Switched to','ja-JP':'切り替えました','ko-KR':'전환됨'}[currentLanguage]} ${packName(active)}`;
    lastTouchSeen = null;
  } else if (selection.phase === 'failed') {
    pendingSelectionId = null;
    message.textContent = `${selection.error || t('切换失败')} · ${packName(active)}`;
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
  const button = document.querySelector('#import'); button.disabled = true; message.textContent = t('正在检查和复制角色包…');
  try { const selected = await invoke('import_pack', {path: document.querySelector('#pack-path').value.trim()}); pendingSelectionId = selected.id; await updatePacks(selected.path); message.textContent = t('已导入，正在尝试切换角色…'); showSelection(await invoke('preferences')); }
  catch (error) {message.textContent = String(error);}
  finally {button.disabled = false;}
});
document.querySelector('#switch').addEventListener('click', async () => {
  try { if (!packList.value) return; const selected = await invoke('select_pack',{path:packList.value}); pendingSelectionId = selected.id; message.textContent = t('正在尝试切换角色…'); showSelection(await invoke('preferences')); }
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
  externalMessage.textContent = p.external_error || (p.external_snap ? t('他应用窗口吸附已开启。') : t(externalHint));
}
externalSnap.addEventListener('change', async () => {
  try { await invoke('set_external_snap', {enabled: externalSnap.checked}); }
  catch (error) { externalMessage.textContent = String(error); externalSnap.checked = false; }
});
scale.addEventListener('input', () => document.querySelector('#scale-value').textContent = `${scale.value}%`);
scale.addEventListener('change', async () => {
  try {await invoke('set_scale',{scale:Number(scale.value)});} catch (error) {message.textContent = String(error);}
});
gazeRadius.addEventListener('input', () => { gazeRadiusValue.textContent = `${gazeRadius.value} ${t('点')}`; });
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
  try { const p = await invoke('preferences'); await updatePacks(p.active_pack); showSelection(p); scale.value = p.scale; gazeRadius.value = p.gaze_radius; gazeRadiusValue.textContent = `${p.gaze_radius} ${t('点')}`; perch.value = p.window_perch; perchValue.textContent = `${p.window_perch}%`; showExternalPreference(p); document.querySelector('#scale-value').textContent = `${p.scale}%`; }
  catch (error) {message.textContent=String(error);}
})();
setInterval(async () => {
  try { const p = await invoke('preferences'); showSelection(p); if (p.error && pendingSelectionId === null && !p.selection_status) message.textContent = p.error; showExternalPreference(p); if (!gazeEditing) { gazeRadius.value = p.gaze_radius; gazeRadiusValue.textContent = `${p.gaze_radius} ${t('点')}`; } if (!perchEditing) { perch.value = p.window_perch; perchValue.textContent = `${p.window_perch}%`; } }
  catch (_) {}
},1000);
