# CLAUDE.md — AutoPing Desktop

Guia de contexto para desenvolvimento neste repositório.

## Visão geral

App desktop Windows (Tauri v2 + Rust) que roda na **conta de usuário** do Discord (self-bot, via token pessoal), monitora **um canal** e, quando um **novo tópico (thread)** é criado nele, envia automaticamente uma mensagem pré-definida dentro do tópico. Fica na bandeja do Windows; janela pequena e ocultável.

### Regra de negócio central

- Tópicos no canal monitorado representam **vagas/oportunidades**. **Quem responde primeiro fica com a vaga.** → **latência é a métrica nº 1**.
- **A resposta SEMPRE aparece depois da 1ª mensagem do criador.** Ao criar um tópico, o usuário já define a mensagem inicial; o Discord emite THREAD_CREATE e só depois MESSAGE_CREATE dessa mensagem. Responder no THREAD_CREATE faria nossa msg aparecer ANTES — suspeito, proibido. **Não existe modo imediato.**
- Gatilho de envio = MESSAGE_CREATE do criador (`author.id == owner_id`) dentro do tópico. A ordem é garantida pelo snowflake (nosso ID é sempre maior).
- Sem msg do criador em `waitForMessageMs` → envia mesmo assim (ex.: tópico criado a partir de mensagem existente).
- **Modo humano**: espera `humanizedDelayMs` após a msg do criador (só aparência; 0 = mais rápido).
- **Status**: `on` (verde, responde) / `off` (vermelho, manual) / `sleep` (azul, entra sozinho após responder se `sleepAfterReply`). Off/Sleep bloqueiam envio e cancelam pendentes. Sleep volta com "Acordar".
- **Som/alarme**: após enviar, "ding" (`soundOnReply`) ou, com `alarmOnReply`, alarme em loop até o usuário parar (o usuário precisa agir de verdade após conseguir a vaga e pode estar longe do PC).
- **Modos de ping** (`mode`): definem QUANDO/O QUE responder. Hoje só `thread` (Tópico). Arquitetura pronta para novos modos.
- **Features gerais** (valem para qualquer modo): sleep após responder, som/alarme ao responder e **alerta de palavras** — mensagem no **canal observado** (não nos tópicos) que *contém* alguma palavra/frase da lista → som ou alarme. Comparação normalizada (`PinG` = `ping` = `PÍNG`: NFKD sem acentos, minúsculas, espaços colapsados). **Independente do status** (alerta mesmo Off/Sleep). Ignora mensagens da própria conta; cooldown 2s.
- App roda **enquanto o usuário joga** → **leveza** é requisito: janela oculta = WebView destruída (~12MB, 1 processo, CPU ~0).

> ⚠️ Self-bots violam os ToS do Discord (risco de ban). O token dá acesso total à conta — nunca logar, commitar, exibir ou enviar a terceiros. Não adicionar libs de selfbot de terceiros (risco de roubo de token).

## Stack

- **Tauri v2** (Rust) + UI em **HTML/CSS/JS vanilla** (`ui/`, sem framework/bundler, `withGlobalTauri`). WebView2.
- Core Rust: `tokio`, `tokio-tungstenite` (gateway), `reqwest` 0.12 (REST, pool keep-alive), `serde_json` (`raw_value`), `keyring` 3 (Cofre do Windows), `windows-sys` (PlaySound), `tauri-plugin-single-instance`.
- **Sem biblioteca de Discord**: cliente próprio (a antiga `discord.js-selfbot-v13` foi arquivada em out/2025; `selfbot.js-v14` é malware/placeholder do npm).
- Toolchain: Rust stable MSVC (`%USERPROFILE%\.cargo\bin`), VS 2022 Build Tools (C++), Node só para o CLI do Tauri (`@tauri-apps/cli`).

## Estrutura

```
src-tauri/
  src/lib.rs              # setup Tauri, comandos IPC, single-instance, run loop (não sai ao fechar janela)
  src/main.rs             # entry (windows_subsystem em release)
  src/engine.rs           # orquestra ModeRunner + features + Gateway + REST + Cache; config ao vivo; alertas; snapshot p/ UI; migração .env
  src/config.rs           # Config (geral + mode/modes + keywordAlert), ConfigPatch, migração do JSON v3.0, validações
  src/secrets.rs          # token no Cofre do Windows (keyring)
  src/audio.rs            # ding/alarme gerados em código (WAV) + PlaySound
  src/logger.rs           # log em arquivo com rotação (logf!)
  src/tray.rs             # ícone colorido por status, pisca laranja no alarme, menu
  src/window.rs           # janela sem borda, sempre por cima, canto inferior direito; ocultar = destroy
  src/core/text.rs        # normalize(): base das comparações de texto
  src/core/reply.rs       # ReplyGate: envio comum a todos os modos (status On, dedupe 30s, sleep após responder); trait Sender
  src/core/modes/mod.rs   # trait Mode, ThreadEvent/MessageEvent, create(), ModeRunner (modo ativo, troca ao vivo)
  src/core/modes/thread/  # modo Tópico (timers, humanizado, timeout) + humanized.rs (pendentes + buffer TTL 15s)
  src/core/features/keyword_alert.rs # alerta de palavras (puro, testável)
  src/discord/gateway.rs  # gateway de usuário: heartbeat, identify, resume, op 7/9, backoff, op 37; parse preguiçoso
  src/discord/rest.rs     # send_message (nonce, retry 1x em 429), get_channel, ping; SharedRest (troca ao vivo)
  src/discord/cache.rs    # guilds/canais (READY/GUILD_*/CHANNEL_*) p/ lista de canais
  src/discord/properties.rs # fingerprint único: IDENTIFY = User-Agent/X-Super-Properties
  tauri.conf.json, capabilities/default.json, icons/, app-icon.png (fonte dos ícones)
ui/index.html, style.css, app.js   # tela principal + configurações em abas (Geral | Modo | Alertas); registro MODES no app.js
docs/screenshots/                  # imagens do README (geradas por mock, ver Release/CI)
```

### Como adicionar um modo de ping
1. Variante em `config::ModeKind` + struct de settings em `config::ModesConfig` (+ patch em `ConfigPatch`).
2. Módulo `core/modes/<modo>/` implementando `Mode` (eventos → `gate.send(...)`; nunca enviar direto).
3. Registrar em `core::modes::create()`.
4. Entrada em `MODES` no `ui/app.js` (label, description, fields, bind/fill/read, summary, chips, isConfigured).
5. Testes com `ModeRunner` + `Sender` mock (`#[tokio::test(start_paused = true)]`).

## Comandos

```bash
npm install
npm run dev        # tauri dev (hot reload: Rust recompila sozinho, UI recarrega)
npm test           # cargo test --lib (core: config, humanized, bot, cache, audio)
npm run build      # release: src-tauri/target/release/autoping.exe + bundle/nsis/*-setup.exe
```
No PowerShell, se `cargo` não for achado: `$env:Path = "$env:USERPROFILE\.cargo\bin;$env:Path"`.

### Release / CI
- `.github/workflows/ci.yml`: `cargo test --lib` em push no `master` e PRs (windows-latest).
- `.github/workflows/release.yml`: tag `v*` → `tauri-action` compila e publica a release com o instalador NSIS. Antes da tag, atualizar `version` em `src-tauri/tauri.conf.json`, `src-tauri/Cargo.toml` e `package.json`.
- Screenshots do README (`docs/screenshots/`): gerados renderizando `ui/` no Edge headless com um mock de `window.__TAURI__` e dados fictícios (nunca usar prints reais com dados da conta).
- Licença MIT.

## Dados do usuário

- Config: `%APPDATA%\com.autoping.desktop\config.json`. **Sem token.** Schema (v3.1):
  ```json
  { "channelId": "", "status": "on", "sleepAfterReply": false, "soundOnReply": true, "alarmOnReply": false,
    "mode": "thread",
    "modes": { "thread": { "autoMessage": "", "waitForMessageMs": 5000, "humanizedEnabled": true, "humanizedDelayMs": 900 } },
    "keywordAlert": { "enabled": false, "keywords": [], "sound": "ding" } }
  ```
  JSON da v3.0 (campos planos `autoMessage`, `soundEnabled`...) é migrado automaticamente no load e regravado.
- Token: Cofre do Windows (Credential Manager), serviço `AutoPing`, usuário `discord-token`.
- Log: `%APPDATA%\com.autoping.desktop\logs\autoping.log` (rotação 1MB). Nunca com token.
- Migração: se o cofre estiver vazio, `Engine::initial_token` importa `DISCORD_TOKEN`/`CHANNEL_ID`/`AUTO_MESSAGE` de um `.env` (pasta atual, pai ou do .exe).

## Arquitetura

### Fluxo
`gateway` (task tokio) → frame `{op,s,t,d: RawValue}` → `GatewayEvents::on_dispatch`:
- MESSAGE_CREATE → `MessageLite{channel_id, author.id, content: Cow}` → **`ModeRunner` primeiro (caminho quente)** → depois `on_keyword_message` (só se `keywords_active`, AtomicBool sem lock).
- THREAD_CREATE → `ModeRunner::on_thread_create`.
- READY/GUILD_*/CHANNEL_* → cache (READY também guarda `self_id`). Demais eventos descartados sem desserializar `d`.

### Caminho quente (modo Tópico)
THREAD_CREATE (`newly_created`, `parent_id == channelId`, status On) → `Humanized` pendente + timer timeout. MESSAGE_CREATE do criador → cancela timeout → `schedule_send` (delay 0 = `tokio::spawn` direto) → `ReplyGate::send` (status On + dedupe) → `SharedRest` → POST `/api/v9/channels/{id}/messages`. Logs/eventos/som só depois do POST. Prazos dos timers calculados no momento do evento (`sleep_until`).

### ReplyGate / ModeRunner
- `ReplyGate` (`core/reply.rs`): status vivo, dedupe, sleep (status → Sleep antes do await + `on_block` cancela o modo; falha → volta On). Notifica `BotEvent` (StatusChanged/Replied/Info) fora de locks.
- `ModeRunner`: só repassa eventos se `gate.is_on()`; `set_status(Off/Sleep)` → `on_block` → `mode.cancel_all()`; `apply(cfg)` troca de modo (cancela o anterior) ou aplica settings ao vivo (canal novo cancela pendentes).

### Engine (`engine.rs`)
- `Arc::new_cyclic` (ReplyGate notifica Engine via `Weak`). `cfg: Mutex<Config>` é a fonte da verdade; status vivo vem do gate (`Engine::config()` junta os dois).
- Alertas unificados: `raise_alert(Ding|Alarm, source: Reply|Keyword, reason)`. Alarm → `rt.alert = Some(ActiveAlert)` + loop até `stop_alarm`; Ding não interrompe alarme tocando. Desligar a origem nas configurações para o alarme.
- `update_config(patch)`: valida; canal novo → `GET /channels/:id` (tipo 0/5/15/16) → cancela pendentes + op 37 subscribe; aplica ao vivo; persiste. Token → `set_token` → cofre + reconecta.
- `snapshot()` → UI (`state-changed`) e tray a cada mudança.
- Keep-warm: ping REST a cada 30s.

### UI / janela / bandeja
- Janela 340×(auto, mín. 170) principal / 560 config, sem borda, transparente, sempre por cima, sem taskbar. **Ocultar/fechar = `destroy()`** (libera WebView2). Sair só pelo menu da bandeja (`RunEvent::ExitRequested` com `code: None` é bloqueado).
- 1º uso (sem token/canal) abre a config; configurado inicia só na bandeja.
- Tray: verde/vermelho/azul; anel vazado = sem conexão; pisca laranja com alarme; clique esquerdo abre/oculta (ou para o alarme).
- Comandos IPC: `get_snapshot`, `update_config`, `set_token`, `set_status`, `list_channels`, `stop_alarm`, `test_sound`, `hide_window`, `resize_window`.
- Snapshot: `status, connection, hasToken, user, channel, mode, thread, sleepAfterReply, soundOnReply, alarmOnReply, keywordAlert, alert{source,reason}|null, lastReply, lastKeywordHit`.
- Tela principal: altura automática (JS mede e chama `resize_window`); config 560px em abas.

## Convenções

- Código, comentários, logs e UI em **PT-BR**.
- Caminho quente: **enviar primeiro, logar depois**; nada de I/O/await antes do POST.
- Toda mudança de config passa por `Engine::update_config` / `Bot::update_config` (aplica ao vivo, persiste, emite).
- `properties.rs` é a única fonte do fingerprint.
- Testes de timer com `#[tokio::test(start_paused = true)]`.
- Nunca logar/exibir o token; ao inspecionar `.env`, filtrar a linha do token.

## Pendências / riscos

- Fingerprint (`CLIENT_BUILD_NUMBER`, versão do Chrome em `properties.rs`) envelhece — atualizar periodicamente.
- Payload READY de usuário varia com `CAPABILITIES`; `cache.rs` faz parse defensivo.
- op 37 em servidores grandes: validar quando trocar de servidor.
- 1 conta por instância (single-instance).

## Histórico

- v1/v2: bot Node no console (+ Docker). v2 trocou `discord.js-selfbot-v13` por cliente próprio (`ws`+`undici`). Código Node removido na v3 (ver histórico git).
- v3.0: app desktop Tauri/Rust.
- v3.1: modos de ping modulares (trait Mode/ModeRunner/ReplyGate), alerta de palavras, config em abas, schema de config aninhado.
