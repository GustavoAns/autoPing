// AutoPing — interface. Vanilla JS, sem dependências.
// Fala com o core Rust via comandos (invoke) e escuta 'state-changed'.

const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;

const CONFIG_HEIGHT = 500;
const MIN_MAIN_HEIGHT = 170;

const $ = (id) => document.getElementById(id);
const esc = (s) => String(s ?? '').replace(/[&<>"']/g, (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' })[c]);

let snap = null;
let view = 'main';

const STATUS_LABEL = { on: 'Ligado', off: 'Desligado', sleep: 'Sleep' };
const CONN_HINT = {
  connecting: 'Conectando…',
  reconnecting: 'Reconectando…',
  invalid_token: 'Token inválido — abra as configurações',
  no_token: 'Sem token — abra as configurações',
  error: 'Erro de conexão',
};

// ═══════════════════════════════════════════════════════════════
// MODOS DE PING
// Cada modo registra: rótulo, descrição, campos da config, como ler/
// preencher esses campos, o resumo da tela principal e seus chips.
// Para um modo novo: adicionar uma entrada aqui (e no core Rust).
// ═══════════════════════════════════════════════════════════════
const MODES = {
  thread: {
    label: 'Tópico',
    description: 'Responde novos tópicos do canal observado, logo após a 1ª mensagem do criador.',
    fields: `
      <label class="field">
        <span class="key">Mensagem <small class="counter" id="f-count">0/2000</small></span>
        <textarea id="f-message" rows="2" maxlength="2000"></textarea>
      </label>
      <label class="field">
        <span class="key">Aguardar 1ª msg do criador (ms)</span>
        <input type="number" id="f-wait" min="500" step="100" />
      </label>
      <div class="opt" data-opt="human">
        <div class="opt-head">
          <button type="button" class="opt-title"><span class="chev"></span><span class="key">Modo humano</span><small class="opt-sum"></small></button>
          <input type="checkbox" class="switch" id="f-human" />
        </div>
        <div class="opt-body">
          <small>Espera extra após a mensagem do criador, para parecer digitado. Desligado = mais rápido.</small>
          <div class="inline">
            <input type="number" id="f-delay" min="0" max="60000" step="50" class="small" />
            <span class="unit">ms</span>
          </div>
        </div>
      </div>`,
    bind() {
      const counter = () => ($('f-count').textContent = `${$('f-message').value.length}/2000`);
      const human = () => ($('f-delay').disabled = !$('f-human').checked);
      $('f-message').addEventListener('input', counter);
      $('f-human').addEventListener('change', human);
      this._sync = () => { counter(); human(); };
    },
    summaries: {
      human: () => ($('f-human').checked ? `${Number($('f-delay').value) || 0}ms` : ''),
    },
    fill(s) {
      const t = s.thread || {};
      $('f-message').value = t.autoMessage || '';
      $('f-wait').value = t.waitForMessageMs ?? 5000;
      $('f-human').checked = !!t.humanizedEnabled;
      $('f-delay').value = t.humanizedDelayMs ?? 900;
      this._sync?.();
    },
    read() {
      return {
        thread: {
          autoMessage: $('f-message').value,
          waitForMessageMs: Number($('f-wait').value) || 0,
          humanizedEnabled: $('f-human').checked,
          humanizedDelayMs: Number($('f-delay').value) || 0,
        },
      };
    },
    isConfigured: (s) => !!s.thread?.autoMessage,
    summary: (s) => [{ key: 'Mensagem', value: s.thread?.autoMessage, mono: true, empty: 'Não configurada' }],
    chips: (s) => [
      { text: s.thread?.humanizedEnabled ? `Humano · ${s.thread.humanizedDelayMs}ms` : 'Humano off', on: !!s.thread?.humanizedEnabled, color: 'on' },
    ],
  },
};

const modeOf = (s) => MODES[s?.mode] || MODES.thread;

// ═══════════════════════════════════════════════════════════════
// TELA PRINCIPAL
// ═══════════════════════════════════════════════════════════════
function chip({ text, on, color }) {
  return `<span class="chip${on ? ' active' : ''}" data-color="${color || 'on'}">${esc(text)}</span>`;
}

function renderMain() {
  if (!snap) return;
  const mode = modeOf(snap);

  const st = $('status');
  st.dataset.status = snap.status;
  st.classList.toggle('offline', snap.connection !== 'connected');
  $('status-label').textContent = STATUS_LABEL[snap.status];

  // Canal
  const ch = $('m-channel');
  ch.classList.remove('warn');
  if (!snap.channel.id) {
    ch.textContent = 'Não configurado';
    ch.classList.add('warn');
  } else if (snap.channel.error) {
    ch.textContent = snap.channel.error;
    ch.classList.add('warn');
  } else if (snap.channel.name) {
    ch.innerHTML = `#${esc(snap.channel.name)}${snap.channel.guildName ? `<span class="sub"> · ${esc(snap.channel.guildName)}</span>` : ''}`;
  } else {
    ch.textContent = snap.channel.id;
  }
  ch.title = ch.textContent;

  // Linhas do modo
  $('m-mode-rows').innerHTML = mode
    .summary(snap)
    .map(
      (r) => `<div class="row"><span class="key">${esc(r.key)}</span>
        <span class="val${r.mono ? ' mono' : ''}${r.value ? '' : ' warn'}" title="${esc(r.value)}">${esc(r.value || r.empty)}</span></div>`,
    )
    .join('');

  // Chips: modo + do modo + gerais
  const kw = snap.keywordAlert || {};
  const chips = [
    { text: mode.label, on: true, color: 'mode' },
    ...mode.chips(snap),
    { text: snap.sleepAfterReply ? 'Sleep' : 'Sleep off', on: snap.sleepAfterReply, color: 'sleep' },
    { text: snap.alarmOnReply ? 'Alarme' : snap.soundOnReply ? 'Som' : 'Mudo', on: snap.alarmOnReply || snap.soundOnReply, color: 'alarm' },
  ];
  if (kw.enabled) chips.push({ text: `🔑 ${kw.keywords.length} palavra${kw.keywords.length === 1 ? '' : 's'}`, on: true, color: 'alarm' });
  $('m-chips').innerHTML = chips.map(chip).join('');

  // Rodapé
  const hint = $('m-hint');
  hint.classList.remove('warn', 'alarm');
  const fmtTime = (ms) => new Date(ms).toLocaleTimeString('pt-BR', { hour: '2-digit', minute: '2-digit' });
  if (snap.alert) {
    hint.textContent = `🔔 ${snap.alert.reason}`;
    hint.classList.add('alarm');
  } else if (snap.connection !== 'connected') {
    hint.textContent = CONN_HINT[snap.connection] || '';
    hint.classList.toggle('warn', ['invalid_token', 'no_token', 'error'].includes(snap.connection));
  } else if (snap.lastKeywordHit && (!snap.lastReply || snap.lastKeywordHit.atUnixMs > snap.lastReply.atUnixMs)) {
    hint.textContent = `🔑 "${snap.lastKeywordHit.keyword}" às ${fmtTime(snap.lastKeywordHit.atUnixMs)}`;
  } else if (snap.lastReply) {
    const ms = snap.lastReply.responseMs;
    hint.textContent = `Última resposta ${fmtTime(snap.lastReply.atUnixMs)} · ${ms >= 1000 ? (ms / 1000).toFixed(1).replace('.', ',') + 's' : ms + 'ms'}`;
  } else {
    hint.textContent = snap.user ? `Conectado como ${snap.user}` : 'Conectado';
  }

  const btn = $('btn-toggle');
  btn.dataset.variant = snap.status;
  if (snap.alert) {
    btn.textContent = 'Parar alarme';
    btn.dataset.action = 'stop-alarm';
  } else if (snap.status === 'on') {
    btn.textContent = 'Desligar';
    btn.dataset.action = 'off';
  } else {
    btn.textContent = snap.status === 'sleep' ? 'Acordar' : 'Ligar';
    btn.dataset.action = 'on';
  }

  fitMainHeight();
}

// A altura da tela principal acompanha o conteúdo (chips podem quebrar linha).
let lastHeight = 0;
function fitMainHeight() {
  if (view !== 'main') return;
  const v = $('view-main');
  const h = Math.max(
    MIN_MAIN_HEIGHT,
    Math.ceil(v.querySelector('.bar').offsetHeight + $('main-body').scrollHeight + v.querySelector('.foot').offsetHeight + 2),
  );
  if (h !== lastHeight) {
    lastHeight = h;
    invoke('resize_window', { height: h });
  }
}

$('btn-toggle').addEventListener('click', async () => {
  const btn = $('btn-toggle');
  btn.disabled = true;
  try {
    snap = btn.dataset.action === 'stop-alarm'
      ? await invoke('stop_alarm')
      : await invoke('set_status', { status: btn.dataset.action });
    renderMain();
  } finally {
    btn.disabled = false;
  }
});

// ═══════════════════════════════════════════════════════════════
// CONFIGURAÇÕES
// ═══════════════════════════════════════════════════════════════
let channels = [];

function selectTab(name) {
  document.querySelectorAll('.tab').forEach((t) => t.classList.toggle('active', t.dataset.tab === name));
  document.querySelectorAll('.pane').forEach((p) => (p.hidden = p.dataset.pane !== name));
}
document.querySelectorAll('.tab').forEach((t) => t.addEventListener('click', () => selectTab(t.dataset.tab)));

function fillChannelSelect(selectedId) {
  const sel = $('f-channel-select');
  sel.innerHTML = '';
  sel.append(new Option(channels.length ? 'Selecionar canal…' : 'Lista indisponível (use o ID)', ''));
  let group = null;
  let lastGuild = null;
  for (const c of channels) {
    if (c.guildName !== lastGuild) {
      group = document.createElement('optgroup');
      group.label = c.guildName;
      sel.append(group);
      lastGuild = c.guildName;
    }
    const prefix = c.kind === 15 ? '📋 ' : c.kind === 5 ? '📢 ' : '# ';
    group.append(new Option(`${prefix}${c.name}${c.category ? `  ·  ${c.category}` : ''}`, c.id));
  }
  sel.value = channels.some((c) => c.id === selectedId) ? selectedId : '';
}

async function loadChannels() {
  try {
    channels = await invoke('list_channels');
  } catch {
    channels = [];
  }
  fillChannelSelect($('f-channel').value.trim());
}

function renderModeFields(modeKey) {
  const mode = MODES[modeKey] || MODES.thread;
  $('f-mode-desc').textContent = mode.description;
  $('f-mode-fields').innerHTML = mode.fields;
  mode.bind();
  mode.fill(snap || {});
  updateSummaries();
}

// ── Opções colapsáveis ─────────────────────────────────────────
// Toda opção opcional é um cartão `.opt`: título + resumo + chave; o corpo
// (detalhes) abre/fecha clicando no título. Começam recolhidas.
const GENERAL_SUMMARIES = {
  keywords: () => {
    const n = parseKeywords().length;
    const sound = document.querySelector('input[name="f-kw-sound"]:checked')?.value === 'alarm' ? 'Alarme' : 'Som';
    return $('f-kw-enabled').checked ? `${n} palavra${n === 1 ? '' : 's'} · ${sound}` : '';
  },
};

function updateSummaries() {
  const modeSummaries = (MODES[$('f-mode').value] || MODES.thread).summaries || {};
  document.querySelectorAll('#form .opt').forEach((card) => {
    const fn = modeSummaries[card.dataset.opt] || GENERAL_SUMMARIES[card.dataset.opt];
    const sum = card.querySelector('.opt-sum');
    if (sum) sum.textContent = fn ? fn() : '';
  });
}

function collapseAll() {
  document.querySelectorAll('#form .opt').forEach((c) => c.classList.remove('open'));
}

$('form').addEventListener('click', (e) => {
  const title = e.target.closest('.opt-title');
  if (title) title.closest('.opt').classList.toggle('open');
});
$('form').addEventListener('input', updateSummaries);
$('form').addEventListener('change', updateSummaries);

// Ligou o alerta sem palavras: abre o cartão para preencher.
$('f-kw-enabled').addEventListener('change', (e) => {
  if (e.target.checked && parseKeywords().length === 0) {
    e.target.closest('.opt').classList.add('open');
    $('f-kw-list').focus();
  }
});

function parseKeywords() {
  return $('f-kw-list').value.split('\n').map((s) => s.trim()).filter(Boolean);
}

function syncKeywordFields() {
  const n = parseKeywords().length;
  $('f-kw-count').textContent = `${n} palavra${n === 1 ? '' : 's'}`;
}

function showError(msg) {
  const el = $('f-error');
  el.textContent = msg || '';
  el.hidden = !msg;
}

function openConfig(tab = 'general') {
  view = 'config';
  const s = snap || {};
  // Geral
  $('f-token').value = '';
  $('f-token').placeholder = s.hasToken ? '•••••••• (deixe vazio para manter)' : 'Cole o token da sua conta';
  $('f-token-hint').textContent = s.hasToken ? 'Token salvo no Cofre do Windows.' : 'Será salvo no Cofre do Windows.';
  $('f-channel').value = s.channel?.id || '';
  $('f-sleep').checked = !!s.sleepAfterReply;
  // Modo
  const sel = $('f-mode');
  sel.innerHTML = Object.entries(MODES).map(([k, m]) => `<option value="${k}">${esc(m.label)}</option>`).join('');
  sel.value = MODES[s.mode] ? s.mode : 'thread';
  renderModeFields(sel.value);
  // Alertas
  $('f-sound').checked = s.soundOnReply ?? true;
  $('f-alarm').checked = !!s.alarmOnReply;
  const kw = s.keywordAlert || {};
  $('f-kw-enabled').checked = !!kw.enabled;
  $('f-kw-list').value = (kw.keywords || []).join('\n');
  document.querySelectorAll('input[name="f-kw-sound"]').forEach((r) => (r.checked = r.value === (kw.sound || 'ding')));
  syncKeywordFields();
  collapseAll();
  updateSummaries();

  showError('');
  fillChannelSelect($('f-channel').value);
  loadChannels();
  selectTab(tab);

  $('view-main').hidden = true;
  $('view-config').hidden = false;
  lastHeight = 0;
  invoke('resize_window', { height: CONFIG_HEIGHT });
}

function openMain() {
  view = 'main';
  $('view-config').hidden = true;
  $('view-main').hidden = false;
  lastHeight = 0;
  renderMain();
}

$('f-mode').addEventListener('change', (e) => renderModeFields(e.target.value));
$('f-channel-select').addEventListener('change', (e) => {
  if (e.target.value) $('f-channel').value = e.target.value;
});
$('f-channel').addEventListener('input', () => fillChannelSelect($('f-channel').value.trim()));
$('f-kw-list').addEventListener('input', syncKeywordFields);
$('btn-refresh').addEventListener('click', loadChannels);
$('btn-settings').addEventListener('click', () => openConfig());
$('btn-back').addEventListener('click', openMain);
$('btn-cancel').addEventListener('click', openMain);
document.querySelectorAll('[data-test]').forEach((b) =>
  b.addEventListener('click', (e) => {
    e.preventDefault();
    invoke('test_sound', { kind: b.dataset.test });
  }),
);

$('form').addEventListener('submit', async (e) => {
  e.preventDefault();
  showError('');
  const save = $('btn-save');
  save.disabled = true;
  save.textContent = 'Salvando…';
  try {
    const token = $('f-token').value.trim();
    if (token) {
      snap = await invoke('set_token', { token });
      $('f-token').value = '';
    }
    const modeKey = $('f-mode').value;
    snap = await invoke('update_config', {
      patch: {
        channelId: $('f-channel').value.trim(),
        sleepAfterReply: $('f-sleep').checked,
        soundOnReply: $('f-sound').checked,
        alarmOnReply: $('f-alarm').checked,
        mode: modeKey,
        ...MODES[modeKey].read(),
        keywordAlert: {
          enabled: $('f-kw-enabled').checked,
          keywords: parseKeywords(),
          sound: document.querySelector('input[name="f-kw-sound"]:checked')?.value || 'ding',
        },
      },
    });
    openMain();
  } catch (err) {
    showError(String(err));
    // Abre o cartão relacionado ao erro, se houver.
    if (/palavra/i.test(String(err))) {
      selectTab('alerts');
      document.querySelector('.opt[data-opt="keywords"]')?.classList.add('open');
    }
  } finally {
    save.disabled = false;
    save.textContent = 'Salvar';
  }
});

// ═══════════════════════════════════════════════════════════════
// GERAL
// ═══════════════════════════════════════════════════════════════
document.querySelectorAll('[data-hide]').forEach((b) => b.addEventListener('click', () => invoke('hide_window')));
document.addEventListener('keydown', (e) => {
  if (e.key === 'Escape') {
    if (view === 'config') openMain();
    else invoke('hide_window');
  }
});
document.addEventListener('contextmenu', (e) => {
  if (!['INPUT', 'TEXTAREA'].includes(e.target.tagName)) e.preventDefault();
});

listen('state-changed', (e) => {
  snap = e.payload;
  if (view === 'main') renderMain();
});

(async () => {
  snap = await invoke('get_snapshot');
  if (!snap.hasToken || !snap.channel.id || !modeOf(snap).isConfigured(snap)) openConfig();
  else openMain();
})();
