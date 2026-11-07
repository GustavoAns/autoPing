//! Engine: liga modos de ping + features gerais + Gateway + REST + Cache +
//! persistência e expõe uma API para a interface. Toda mudança de config é
//! aplicada AO VIVO (sem reiniciar a conexão, exceto troca de token).

use crate::config::{
    AlertSound, Config, ConfigPatch, KeywordAlertConfig, ModeKind, Status, ThreadPatch, ThreadSettings,
    THREAD_PARENT_TYPES,
};
use crate::core::features::keyword_alert::{self, KeywordAlert};
use crate::core::modes::{MessageEvent, ModeRunner, ThreadEvent};
use crate::core::reply::{unix_ms, BotEvent, Reply};
use crate::discord::cache::{Cache, ChannelEntry, ChannelPayload, GuildDeletePayload, GuildPayload, ReadyPayload};
use crate::discord::gateway::{self, ConnState, GatewayHandle, Handler};
use crate::discord::rest::{Rest, SharedRest};
use crate::{audio, logf, secrets};
use serde::{Deserialize, Serialize};
use serde_json::value::RawValue;
use std::borrow::Cow;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock, Weak};
use std::time::Duration;

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChannelView {
    pub id: String,
    pub name: Option<String>,
    pub guild_name: Option<String>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum AlertSource {
    Reply,
    Keyword,
}

/// Alarme tocando (aguardando o usuário parar).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ActiveAlert {
    pub source: AlertSource,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KeywordHit {
    pub keyword: String,
    pub at_unix_ms: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    pub status: Status,
    pub connection: ConnState,
    pub has_token: bool,
    pub user: Option<String>,
    pub channel: ChannelView,
    pub mode: ModeKind,
    pub thread: ThreadSettings,
    pub sleep_after_reply: bool,
    pub sound_on_reply: bool,
    pub alarm_on_reply: bool,
    pub keyword_alert: KeywordAlertConfig,
    pub alert: Option<ActiveAlert>,
    pub last_reply: Option<Reply>,
    pub last_keyword_hit: Option<KeywordHit>,
}

struct Runtime {
    connection: ConnState,
    has_token: bool,
    user: Option<String>,
    self_id: Option<String>,
    channel: ChannelView,
    last_reply: Option<Reply>,
    last_keyword_hit: Option<KeywordHit>,
    alert: Option<ActiveAlert>,
}

type OnChange = Box<dyn Fn(&Snapshot) + Send + Sync>;

pub struct Engine {
    config_path: PathBuf,
    /// Fonte da verdade da config (o status vivo fica no ReplyGate).
    cfg: Mutex<Config>,
    runner: Arc<ModeRunner>,
    keywords: Mutex<KeywordAlert>,
    /// Atalho sem lock para o caminho quente.
    keywords_active: AtomicBool,
    /// Já recebeu alguma mensagem do canal desde a última ativação (diagnóstico).
    keywords_seen: AtomicBool,
    rest: SharedRest,
    cache: Mutex<Cache>,
    gateway: Mutex<Option<GatewayHandle>>,
    rt: Mutex<Runtime>,
    on_change: OnceLock<OnChange>,
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

impl Engine {
    pub fn new(config_path: PathBuf) -> Arc<Self> {
        let (cfg, migrated) = Config::load(&config_path);
        let rest = SharedRest::default();
        let engine = Arc::new_cyclic(|weak: &Weak<Engine>| {
            let w = weak.clone();
            let notify = Arc::new(move |ev: BotEvent| {
                if let Some(engine) = w.upgrade() {
                    engine.on_bot_event(ev);
                }
            });
            let keywords = KeywordAlert::from_config(&cfg.keyword_alert);
            Engine {
                config_path,
                runner: ModeRunner::new(&cfg, Arc::new(rest.clone()), notify),
                keywords_active: AtomicBool::new(keywords.is_active()),
                keywords_seen: AtomicBool::new(false),
                keywords: Mutex::new(keywords),
                rest,
                cache: Mutex::new(Cache::default()),
                gateway: Mutex::new(None),
                rt: Mutex::new(Runtime {
                    connection: ConnState::NoToken,
                    has_token: false,
                    user: None,
                    self_id: None,
                    channel: ChannelView { id: cfg.channel_id.clone(), ..Default::default() },
                    last_reply: None,
                    last_keyword_hit: None,
                    alert: None,
                }),
                cfg: Mutex::new(cfg),
                on_change: OnceLock::new(),
            }
        });
        if migrated {
            logf!("Configuração migrada para o novo formato (modos de ping)");
            engine.persist();
        }
        engine
    }

    pub fn set_on_change(&self, f: impl Fn(&Snapshot) + Send + Sync + 'static) {
        let _ = self.on_change.set(Box::new(f));
    }

    /// Config atual com o status vivo.
    fn config(&self) -> Config {
        let mut c = lock(&self.cfg).clone();
        c.status = self.runner.status();
        c
    }

    pub fn snapshot(&self) -> Snapshot {
        let cfg = self.config();
        let rt = lock(&self.rt);
        Snapshot {
            status: cfg.status,
            connection: rt.connection,
            has_token: rt.has_token,
            user: rt.user.clone(),
            channel: rt.channel.clone(),
            mode: cfg.mode,
            thread: cfg.modes.thread,
            sleep_after_reply: cfg.sleep_after_reply,
            sound_on_reply: cfg.sound_on_reply,
            alarm_on_reply: cfg.alarm_on_reply,
            keyword_alert: cfg.keyword_alert,
            alert: rt.alert.clone(),
            last_reply: rt.last_reply.clone(),
            last_keyword_hit: rt.last_keyword_hit.clone(),
        }
    }

    fn emit(&self) {
        if let Some(f) = self.on_change.get() {
            f(&self.snapshot());
        }
    }

    fn persist(&self) {
        if let Err(e) = self.config().save(&self.config_path) {
            logf!("Falha ao salvar config: {e}");
        }
    }

    // ── Ciclo de vida ───────────────────────────────────────────────

    /// Token salvo no cofre ou importado do `.env` da versão de console.
    /// Chamado antes de `start` para decidir se a janela abre no 1º uso.
    pub fn initial_token(&self) -> Option<String> {
        secrets::load_token().or_else(|| self.import_legacy_env())
    }

    /// Deve ser chamado dentro do runtime tokio.
    pub fn start(self: &Arc<Self>, token: Option<String>) {
        logf!("AutoPing iniciado (modo {:?})", self.runner.kind());
        if self.keywords_active.load(Ordering::Acquire) {
            logf!("Alerta de palavras ativo ({} palavra(s))", lock(&self.cfg).keyword_alert.keywords.len());
        }
        // Keep-warm: mantém a conexão HTTPS aberta para o envio ser rápido.
        let w = Arc::downgrade(self);
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(Duration::from_secs(30)).await;
                let Some(engine) = w.upgrade() else { return };
                if let Some(rest) = engine.rest.get() {
                    rest.ping().await;
                }
            }
        });
        match token {
            Some(token) => self.connect(token),
            None => {
                logf!("Nenhum token salvo - configure na interface");
                self.emit();
            }
        }
    }

    /// Migração da versão console: se existir um `.env` (pasta atual, pasta do
    /// .exe ou pasta pai) e o cofre estiver vazio, importa token/canal/mensagem.
    /// O token vai direto para o Cofre do Windows; nunca é logado.
    fn import_legacy_env(&self) -> Option<String> {
        let mut dirs: Vec<PathBuf> = Vec::new();
        if let Ok(cwd) = std::env::current_dir() {
            dirs.push(cwd.clone());
            if let Some(p) = cwd.parent() {
                dirs.push(p.to_path_buf());
            }
        }
        if let Some(exe_dir) = std::env::current_exe().ok().and_then(|p| p.parent().map(|p| p.to_path_buf())) {
            dirs.push(exe_dir);
        }
        let content = dirs.iter().find_map(|d| std::fs::read_to_string(d.join(".env")).ok())?;
        let mut vars = std::collections::HashMap::new();
        for line in content.lines() {
            let line = line.trim();
            if line.starts_with('#') {
                continue;
            }
            if let Some((k, v)) = line.split_once('=') {
                vars.insert(k.trim().to_string(), v.trim().trim_matches('"').to_string());
            }
        }
        let token = vars.get("DISCORD_TOKEN").filter(|t| t.len() >= 20 && !t.contains("seu_token"))?.clone();
        secrets::save_token(&token).ok()?;

        let cfg = self.config();
        let patch = ConfigPatch {
            channel_id: vars.get("CHANNEL_ID").filter(|_| cfg.channel_id.is_empty()).cloned(),
            thread: Some(ThreadPatch {
                auto_message: vars
                    .get("AUTO_MESSAGE")
                    .filter(|m| cfg.modes.thread.auto_message.is_empty() && !m.is_empty())
                    .cloned(),
                ..Default::default()
            }),
            ..Default::default()
        };
        if let Ok(next) = cfg.with_patch(&patch) {
            lock(&self.rt).channel.id = next.channel_id.clone();
            self.apply_config(next);
            self.persist();
        }
        logf!("Configuração importada do .env da versão anterior (token salvo no Cofre do Windows)");
        Some(token)
    }

    fn connect(self: &Arc<Self>, token: String) {
        let rest = match Rest::new(&token) {
            Ok(r) => Arc::new(r),
            Err(e) => {
                logf!("{e}");
                lock(&self.rt).connection = ConnState::InvalidToken;
                self.emit();
                return;
            }
        };
        self.rest.set(Some(rest.clone()));
        tokio::spawn(async move { rest.ping().await }); // pré-aquece TLS
        {
            let mut rt = lock(&self.rt);
            rt.has_token = true;
            rt.connection = ConnState::Connecting;
        }
        let handler: Arc<dyn Handler> = Arc::new(GatewayEvents(Arc::downgrade(self)));
        *lock(&self.gateway) = Some(gateway::spawn(token, handler));
        self.emit();
    }

    fn disconnect(&self) {
        if let Some(gw) = lock(&self.gateway).take() {
            gw.shutdown();
        }
        self.rest.set(None);
        let mut rt = lock(&self.rt);
        rt.user = None;
        rt.self_id = None;
        rt.connection = ConnState::NoToken;
    }

    // ── API para a interface ────────────────────────────────────────

    pub fn set_token(self: &Arc<Self>, token: String) -> Result<Snapshot, String> {
        let token = token.trim().trim_matches('"').to_string();
        if token.len() < 20 || token.contains(char::is_whitespace) {
            return Err("Token inválido.".into());
        }
        secrets::save_token(&token)?;
        logf!("Token atualizado - reconectando");
        self.disconnect();
        self.connect(token);
        Ok(self.snapshot())
    }

    pub fn set_status(&self, status: Status) -> Snapshot {
        self.runner.set_status(status); // evento StatusChanged persiste e emite
        self.snapshot()
    }

    /// Para o alarme (botão na janela ou bandeja).
    pub fn stop_alarm(&self) -> Snapshot {
        let was = lock(&self.rt).alert.take();
        audio::stop();
        if was.is_some() {
            logf!("Alarme parado pelo usuário");
            self.emit();
        }
        self.snapshot()
    }

    pub fn is_alarm_ringing(&self) -> bool {
        lock(&self.rt).alert.is_some()
    }

    /// Toca o som para teste na tela de configurações.
    pub fn test_sound(&self, kind: &str) {
        if self.is_alarm_ringing() {
            return;
        }
        match kind {
            "alarm" => {
                audio::alarm();
                tokio::spawn(async {
                    tokio::time::sleep(Duration::from_millis(2800)).await;
                    audio::stop();
                });
            }
            _ => audio::ding(),
        }
    }

    pub fn list_channels(&self) -> Vec<ChannelEntry> {
        lock(&self.cache).list_channels()
    }

    /// Aplica a config em todos os componentes (ao vivo).
    fn apply_config(&self, next: Config) {
        self.runner.apply(&next);
        {
            let mut k = lock(&self.keywords);
            k.apply(&next.keyword_alert);
            self.keywords_active.store(k.is_active(), Ordering::Release);
            self.keywords_seen.store(false, Ordering::Release);
            if k.is_active() {
                logf!("Alerta de palavras ativo ({} palavra(s), {:?})", next.keyword_alert.keywords.len(), next.keyword_alert.sound);
            }
        }
        *lock(&self.cfg) = next;
    }

    pub async fn update_config(self: &Arc<Self>, patch: ConfigPatch) -> Result<Snapshot, String> {
        let current = self.config();
        let next = current.with_patch(&patch)?;
        let channel_changed = next.channel_id != current.channel_id;

        let mut resolved: Option<(ChannelView, Option<String>)> = None;
        if channel_changed {
            resolved = Some(if next.channel_id.is_empty() {
                (ChannelView::default(), None)
            } else {
                self.resolve_channel(&next.channel_id).await?
            });
        }

        self.apply_config(next.clone());
        self.persist();
        self.stop_alarm_if_disabled(&next);
        if let Some((view, guild_id)) = resolved {
            lock(&self.rt).channel = view;
            if let Some(gid) = guild_id {
                self.subscribe(&gid, &next.channel_id);
            }
            logf!("Canal observado alterado para {}", next.channel_id);
        }
        logf!("Configuração atualizada");
        self.emit();
        Ok(self.snapshot())
    }

    /// Se a origem do alarme tocando foi desligada nas configurações, para.
    fn stop_alarm_if_disabled(&self, cfg: &Config) {
        let mut rt = lock(&self.rt);
        let still_wanted = match rt.alert.as_ref().map(|a| a.source) {
            Some(AlertSource::Reply) => cfg.alarm_on_reply,
            Some(AlertSource::Keyword) => cfg.keyword_alert.enabled && cfg.keyword_alert.sound == AlertSound::Alarm,
            None => return,
        };
        if !still_wanted {
            rt.alert = None;
            audio::stop();
        }
    }

    /// Valida o canal via REST. Sem token, aceita o ID sem validar.
    async fn resolve_channel(&self, channel_id: &str) -> Result<(ChannelView, Option<String>), String> {
        let Some(rest) = self.rest.get() else {
            return Ok((ChannelView { id: channel_id.into(), ..Default::default() }, None));
        };
        match rest.get_channel(channel_id).await {
            Ok(ch) => {
                if !THREAD_PARENT_TYPES.contains(&ch.kind) {
                    return Err("Esse canal não suporta tópicos (use texto, fórum, mídia ou anúncios).".into());
                }
                let guild_name = ch.guild_id.as_deref().and_then(|g| lock(&self.cache).guild_name(g)).or(ch.guild_id.clone());
                Ok((ChannelView { id: ch.id, name: ch.name, guild_name, error: None }, ch.guild_id))
            }
            Err(e) if e.status == 404 || e.code == Some(10003) => Err("Canal não encontrado.".into()),
            Err(e) if e.status == 403 || e.code == Some(50001) => Err("Sem acesso a esse canal.".into()),
            Err(e) => Err(format!("Erro ao validar canal: {e}")),
        }
    }

    fn subscribe(&self, guild_id: &str, channel_id: &str) {
        if let Some(gw) = lock(&self.gateway).as_ref() {
            gw.subscribe(guild_id, channel_id);
            logf!("Inscrito nos eventos do canal observado");
        }
    }

    async fn after_ready(self: Arc<Self>) {
        let channel_id = lock(&self.cfg).channel_id.clone();
        if channel_id.is_empty() {
            self.emit();
            return;
        }
        match self.resolve_channel(&channel_id).await {
            Ok((view, guild_id)) => {
                logf!("Observando #{} ({})", view.name.clone().unwrap_or_default(), view.guild_name.clone().unwrap_or_default());
                lock(&self.rt).channel = view;
                if let Some(gid) = guild_id {
                    self.subscribe(&gid, &channel_id);
                }
            }
            Err(e) => {
                logf!("Canal configurado com problema: {e}");
                lock(&self.rt).channel = ChannelView { id: channel_id, error: Some(e), ..Default::default() };
            }
        }
        self.emit();
    }

    // ── Alertas (som / alarme) ──────────────────────────────────────

    /// Toca som ou alarme. Sempre chamado FORA do caminho quente.
    fn raise_alert(&self, sound: AlertSound, source: AlertSource, reason: String) {
        match sound {
            AlertSound::Alarm => {
                logf!("Alarme tocando ({reason}) - aguardando o usuário parar");
                lock(&self.rt).alert = Some(ActiveAlert { source, reason });
                audio::alarm();
            }
            AlertSound::Ding => {
                // Não interrompe um alarme que já está tocando.
                if !self.is_alarm_ringing() {
                    audio::ding();
                }
            }
        }
    }

    fn on_keyword_message(&self, msg: &MessageEvent) {
        let (channel_id, auto_message) = {
            let c = lock(&self.cfg);
            (c.channel_id.clone(), c.modes.thread.auto_message.clone())
        };
        if msg.channel_id != channel_id {
            return;
        }
        // Diagnóstico: confirma (uma vez por ativação) que o canal está chegando.
        if !self.keywords_seen.swap(true, Ordering::AcqRel) {
            logf!("Alerta de palavras: recebendo mensagens do canal observado");
        }
        let self_id = lock(&self.rt).self_id.clone();
        let hit = lock(&self.keywords).check(
            &keyword_alert::Message { channel_id: msg.channel_id, author_id: msg.author_id, content: msg.content },
            &channel_id,
            self_id.as_deref().map(|id| (id, auto_message.as_str())),
            tokio::time::Instant::now(),
        );
        let Some(hit) = hit else { return };
        logf!("Palavra \"{}\" detectada no canal observado", hit.keyword);
        lock(&self.rt).last_keyword_hit = Some(KeywordHit { keyword: hit.keyword.clone(), at_unix_ms: unix_ms() });
        self.raise_alert(hit.sound, AlertSource::Keyword, format!("Palavra \"{}\" detectada!", hit.keyword));
        self.emit();
    }

    // ── Eventos ─────────────────────────────────────────────────────

    fn on_bot_event(&self, ev: BotEvent) {
        match ev {
            BotEvent::StatusChanged(s) => {
                logf!("Status: {s:?}");
                self.persist();
                self.emit();
            }
            BotEvent::Replied(r) => {
                logf!("Mensagem enviada ({}) em {} em {}ms", r.reason, r.thread_id, r.response_ms);
                lock(&self.rt).last_reply = Some(r);
                let (alarm, sound) = {
                    let c = lock(&self.cfg);
                    (c.alarm_on_reply, c.sound_on_reply)
                };
                // Som DEPOIS do envio (fora do caminho quente).
                if alarm {
                    self.raise_alert(AlertSound::Alarm, AlertSource::Reply, "Mensagem enviada!".into());
                } else if sound {
                    self.raise_alert(AlertSound::Ding, AlertSource::Reply, "Mensagem enviada!".into());
                }
                self.emit();
            }
            BotEvent::Info(msg) => logf!("{msg}"),
        }
    }
}

// ── Handler do gateway ──────────────────────────────────────────────

#[derive(Deserialize)]
struct MessageLite<'a> {
    #[serde(borrow)]
    channel_id: &'a str,
    #[serde(borrow)]
    author: AuthorLite<'a>,
    #[serde(borrow, default)]
    content: Cow<'a, str>,
}

#[derive(Deserialize)]
struct AuthorLite<'a> {
    #[serde(borrow)]
    id: &'a str,
}

#[derive(Deserialize)]
struct ThreadLite {
    id: String,
    #[serde(default)]
    parent_id: Option<String>,
    #[serde(default)]
    owner_id: Option<String>,
    #[serde(default)]
    newly_created: bool,
    #[serde(default)]
    name: Option<String>,
}

struct GatewayEvents(Weak<Engine>);

impl Handler for GatewayEvents {
    fn on_dispatch(&self, t: &str, d: &RawValue) {
        let Some(engine) = self.0.upgrade() else { return };
        match t {
            "MESSAGE_CREATE" => {
                if let Ok(m) = serde_json::from_str::<MessageLite>(d.get()) {
                    let msg = MessageEvent { channel_id: m.channel_id, author_id: m.author.id, content: &m.content };
                    // Caminho quente primeiro: modo de ping.
                    engine.runner.on_message_create(&msg);
                    // Depois as features gerais.
                    if engine.keywords_active.load(Ordering::Acquire) {
                        engine.on_keyword_message(&msg);
                    }
                }
            }
            "THREAD_CREATE" => {
                if let Ok(th) = serde_json::from_str::<ThreadLite>(d.get()) {
                    engine.runner.on_thread_create(&ThreadEvent {
                        id: th.id,
                        parent_id: th.parent_id,
                        owner_id: th.owner_id,
                        newly_created: th.newly_created,
                        name: th.name,
                    });
                }
            }
            "READY" => match serde_json::from_str::<ReadyPayload>(d.get()) {
                Ok(r) => {
                    logf!("Logado como {}", r.user.username);
                    {
                        let mut rt = lock(&engine.rt);
                        rt.user = Some(r.user.username.clone());
                        rt.self_id = Some(r.user.id.clone());
                    }
                    lock(&engine.cache).load_ready(r.guilds);
                    tokio::spawn(engine.clone().after_ready());
                }
                Err(e) => logf!("Falha ao ler READY: {e}"),
            },
            "GUILD_CREATE" | "GUILD_UPDATE" => {
                if let Ok(g) = serde_json::from_str::<GuildPayload>(d.get()) {
                    lock(&engine.cache).upsert_guild(g);
                }
            }
            "GUILD_DELETE" => {
                if let Ok(g) = serde_json::from_str::<GuildDeletePayload>(d.get()) {
                    lock(&engine.cache).delete_guild(g);
                }
            }
            "CHANNEL_CREATE" | "CHANNEL_UPDATE" => {
                if let Ok(c) = serde_json::from_str::<ChannelPayload>(d.get()) {
                    lock(&engine.cache).upsert_channel(c);
                }
            }
            "CHANNEL_DELETE" => {
                if let Ok(c) = serde_json::from_str::<ChannelPayload>(d.get()) {
                    lock(&engine.cache).delete_channel(c);
                }
            }
            _ => {}
        }
    }

    fn on_connection(&self, state: ConnState) {
        let Some(engine) = self.0.upgrade() else { return };
        {
            let mut rt = lock(&engine.rt);
            if rt.connection == state {
                return;
            }
            rt.connection = state;
        }
        logf!("Conexão: {state:?}");
        engine.emit();
    }
}
