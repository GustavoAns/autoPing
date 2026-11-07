// AutoPing — interface. Vanilla JS, sem dependências.
// Fala com o core Rust via comandos (invoke) e escuta 'state-changed'.

const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;

const MAIN_HEIGHT = 196;
const CONFIG_HEIGHT = 660;

const $ = (id) => document.getElementById(id);
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

// ── Tela principal ─────────────────────────────────────────────
function renderMain() {
  if (!snap) return;
  const st = $('status');
  st.dataset.status = snap.status;
  st.classList.toggle('offline', snap.connection !== 'connected');
  $('status-label').textContent = STATUS_LABEL[snap.status];

  const ch = $('m-channel');
  ch.classList.remove('warn');
  if (!snap.channel.id) {
    ch.textContent = 'Não configurado';
    ch.classList.add('warn');
  } else if (snap.channel.error) {
    ch.textContent = snap.channel.error;
    ch.classList.add('warn');
  } else if (snap.channel.name) {
    ch.innerHTML = '';
    ch.append(`#${snap.channel.name}`);
    if (snap.channel.guildName) {
      const sub = document.createElement('span');
      sub.className = 'sub';
      sub.textContent = ` · ${snap.channel.guildName}`;
      ch.append(sub);
    }
  } else {
    ch.textContent = snap.channel.id;
  }
  ch.title = ch.textContent;

  const msg = $('m-message');
  msg.textContent = snap.autoMessage || 'Não configurada';
  msg.classList.toggle('warn', !snap.autoMessage);
  msg.title = snap.autoMessage;

  const human = $('m-human');
  human.classList.toggle('active', snap.humanizedEnabled);
  human.textContent = snap.humanizedEnabled ? `Humano · ${snap.humanizedDelayMs}ms` : 'Humano off';

  const sleep = $('m-sleep');
  sleep.classList.toggle('active', snap.sleepAfterReply);
  sleep.textContent = snap.sleepAfterReply ? 'Sleep' : 'Sleep off';

  const sound = $('m-sound');
  sound.classList.toggle('active', snap.alarmEnabled || snap.soundEnabled);
  sound.textContent = snap.alarmEnabled ? 'Alarme' : snap.soundEnabled ? 'Som' : 'Mudo';

  const hint = $('m-hint');
  hint.classList.remove('warn', 'alarm');
  if (snap.alarmRinging) {
    hint.textContent = '🔔 Mensagem enviada!';
    hint.classList.add('alarm');
  } else if (snap.connection !== 'connected') {
    hint.textContent = CONN_HINT[snap.connection] || '';
    hint.classList.toggle('warn', ['invalid_token', 'no_token', 'error'].includes(snap.connection));
  } else if (snap.lastReply) {
    const t = new Date(snap.lastReply.atUnixMs).toLocaleTimeString('pt-BR', { hour: '2-digit', minute: '2-digit' });
    const ms = snap.lastReply.responseMs;
    hint.textContent = `Última resposta ${t} · ${ms >= 1000 ? (ms / 1000).toFixed(1).replace('.', ',') + 's' : ms + 'ms'}`;
  } else {
    hint.textContent = snap.user ? `Conectado como ${snap.user}` : 'Conectado';
  }

  const btn = $('btn-toggle');
  btn.dataset.variant = snap.status;
  if (snap.alarmRinging) {
    btn.textContent = 'Parar alarme';
    btn.dataset.action = 'stop-alarm';
  } else if (snap.status === 'on') {
    btn.textContent = 'Desligar';
    btn.dataset.action = 'off';
  } else {
    btn.textContent = snap.status === 'sleep' ? 'Acordar' : 'Ligar';
    btn.dataset.action = 'on';
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

// ── Configurações ──────────────────────────────────────────────
let channels = [];

function fillChannelSelect(selectedId) {
  const sel = $('f-channel-select');
  sel.innerHTML = '';
  const manual = new Option(channels.length ? 'Selecionar canal…' : 'Lista indisponível (use o ID)', '');
  sel.append(manual);
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

function updateCounter() {
  $('f-count').textContent = `${$('f-message').value.length}/2000`;
}

function syncHumanField() {
  $('f-delay').disabled = !$('f-human').checked;
}

function showError(msg) {
  const el = $('f-error');
  el.textContent = msg || '';
  el.hidden = !msg;
}

function openConfig() {
  view = 'config';
  const s = snap || {};
  $('f-token').value = '';
  $('f-token').placeholder = s.hasToken ? '•••••••• (deixe vazio para manter)' : 'Cole o token da sua conta';
  $('f-token-hint').textContent = s.hasToken ? 'Token salvo no Cofre do Windows.' : 'Será salvo no Cofre do Windows.';
  $('f-channel').value = s.channel?.id || '';
  $('f-message').value = s.autoMessage || '';
  $('f-wait').value = s.waitForMessageMs ?? 5000;
  $('f-human').checked = !!s.humanizedEnabled;
  $('f-delay').value = s.humanizedDelayMs ?? 900;
  $('f-sleep').checked = !!s.sleepAfterReply;
  $('f-sound').checked = s.soundEnabled ?? true;
  $('f-alarm').checked = !!s.alarmEnabled;
  updateCounter();
  syncHumanField();
  showError('');
  fillChannelSelect($('f-channel').value);
  loadChannels();

  $('view-main').hidden = true;
  $('view-config').hidden = false;
  invoke('resize_window', { height: CONFIG_HEIGHT });
}

function openMain() {
  view = 'main';
  $('view-config').hidden = true;
  $('view-main').hidden = false;
  invoke('resize_window', { height: MAIN_HEIGHT });
  renderMain();
}

$('f-channel-select').addEventListener('change', (e) => {
  if (e.target.value) $('f-channel').value = e.target.value;
});
$('f-channel').addEventListener('input', () => fillChannelSelect($('f-channel').value.trim()));
$('f-message').addEventListener('input', updateCounter);
$('f-human').addEventListener('change', syncHumanField);
$('btn-refresh').addEventListener('click', loadChannels);
document.querySelectorAll('[data-test]').forEach((b) =>
  b.addEventListener('click', (e) => {
    e.preventDefault();
    invoke('test_sound', { kind: b.dataset.test });
  }),
);
$('btn-settings').addEventListener('click', openConfig);
$('btn-back').addEventListener('click', openMain);
$('btn-cancel').addEventListener('click', openMain);

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
    snap = await invoke('update_config', {
      patch: {
        channelId: $('f-channel').value.trim(),
        autoMessage: $('f-message').value,
        waitForMessageMs: Number($('f-wait').value) || 0,
        humanizedEnabled: $('f-human').checked,
        humanizedDelayMs: Number($('f-delay').value) || 0,
        sleepAfterReply: $('f-sleep').checked,
        soundEnabled: $('f-sound').checked,
        alarmEnabled: $('f-alarm').checked,
      },
    });
    openMain();
  } catch (err) {
    showError(String(err));
  } finally {
    save.disabled = false;
    save.textContent = 'Salvar';
  }
});

// ── Geral ──────────────────────────────────────────────────────
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
  if (!snap.hasToken || !snap.channel.id || !snap.autoMessage) openConfig();
  else openMain();
})();
