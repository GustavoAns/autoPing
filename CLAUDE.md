# CLAUDE.md — Discord AutoPing

Guia de contexto para desenvolvimento neste repositório.

## Visão geral

Self-bot do Discord (roda na **conta de usuário**, via token pessoal) que monitora **um canal** e, quando um **novo tópico (thread)** é criado nele, envia automaticamente uma mensagem pré-definida dentro do tópico.

### Regra de negócio central

- Tópicos no canal monitorado representam **vagas/oportunidades**. **Quem responde primeiro fica com a vaga.**
- Portanto **latência é a métrica nº 1**. Toda decisão de arquitetura existe para reduzir o tempo entre o evento `THREAD_CREATE` e o `POST` da mensagem.
- A mensagem enviada (ex.: `222/555/666-FB/666-Rep`) é o "claim" do usuário na vaga.
- **A resposta SEMPRE deve aparecer depois da 1ª mensagem do criador.** Ao criar um tópico, o usuário já define a mensagem inicial; o Discord cria o tópico (THREAD_CREATE) e só depois posta essa mensagem (MESSAGE_CREATE). Responder no THREAD_CREATE faz nossa msg aparecer ANTES da do criador — suspeito, proibido. **Não existe modo imediato.**
- Gatilho de envio = MESSAGE_CREATE do criador (`author.id === owner_id`) dentro do tópico. A ordem é garantida pelo snowflake (nosso ID é sempre maior), então `HUMANIZED_DELAY` não é necessário para a ordem; padrão 900ms por aparência humana (0 = mais rápido).
- Lógica isolada em `src/core/humanized.js` (será retrabalhada / exposta na UI).

> ⚠️ Self-bots violam os ToS do Discord (risco de ban da conta). O token dá acesso total à conta — nunca logar, commitar ou enviar o token para terceiros. Não adicionar libs de selfbot de terceiros (risco de roubo de token).

## Stack

- Node.js **>= 22** (dev local: `nvm use 24.15.0`; Docker: `node:24-alpine`), CommonJS.
- **Sem lib de Discord.** Cliente próprio:
  - `ws` — gateway (WebSocket).
  - `undici` — REST (Pool com conexões TLS persistentes).
- `dotenv` — carrega `.env`.
- Testes: `node:test` nativo (`npm test`). Sem linter, sem build.

> Histórico: até a v1 usava `discord.js-selfbot-v13`, arquivada em 11/out/2025. Substituída pelo cliente próprio na v2. **Não** usar `selfbot.js-v14` (placeholder de segurança do npm / malware).

## Estrutura

```
src/index.js               # Wiring: config ↔ gateway ↔ responder ↔ humanized ↔ commands; status; shutdown
src/config.js              # Config (EventEmitter, evento 'change'), loadToken, validateMessage, THREAD_PARENT_TYPES
src/commands.js            # Comandos !autoPing, validateChannel (REST), formatChannelList
src/core/responder.js      # Decide quando enviar; dedupe; checa `enabled` antes de todo envio
src/core/humanized.js      # HumanizedWaiter: pending/scheduled/buffer de msgs adiantadas
src/discord/gateway.js     # Gateway de usuário: heartbeat, identify, resume, reconnect, op 37
src/discord/rest.js        # undici Pool: sendMessage (retry 429), getChannel, deleteMessage, warmup
src/discord/cache.js       # Guilds/canais vindos do gateway (para listar e nomes)
src/discord/properties.js  # Fingerprint: properties do IDENTIFY = User-Agent/X-Super-Properties do REST
src/utils/listChannels.js  # Script: conecta, lista canais com IDs, sai
test/                      # node:test (humanized + responder)
docker-compose.yml         # 1 conta (.env da raiz)
docker-compose.multi.yml   # N contas (instances/<conta>/.env)
```

## Comandos de desenvolvimento

```bash
nvm use 24.15.0
npm install
npm test                 # testes unitários (sem token)
npm start                # node src/index.js
npm run dev              # node --watch
npm run list-channels    # lista canais e IDs (precisa só do DISCORD_TOKEN)

docker-compose up -d --build
docker-compose -f docker-compose.multi.yml up -d --build
```

## Configuração (`.env`)

| Variável | Obrigatória | Default | Uso |
|---|---|---|---|
| `DISCORD_TOKEN` | sim | — | Token da conta. Processo aborta se vazio ou contiver `seu_token`. |
| `CHANNEL_ID` | não* | `''` | Canal pai monitorado (texto 0, anúncio 5, fórum 15, mídia 16). |
| `AUTO_MESSAGE` | não* | `''` | Mensagem enviada. Sem ela, nada é enviado. |
| `WAIT_FOR_MESSAGE` | não | `5000` | ms máx. aguardando 1ª msg do criador; depois envia mesmo assim (ex.: tópico criado a partir de msg existente). `0`/vazio = 5000. |
| `HUMANIZED_DELAY` | não | `900` | Delay humanizado após a 1ª msg do criador. `0` = envio síncrono, mais rápido. |
| `CLIENT_BUILD_NUMBER` | não | ver `properties.js` | Build number do cliente web no fingerprint. |

\* Podem ser definidos em runtime via comando, mas nada é enviado sem eles.

Alterações via comando são **em memória** — perdidas ao reiniciar.

## Arquitetura

### Fluxo de eventos
`gateway 'dispatch' (t, d)` → **`responder.handleDispatch` primeiro (caminho quente)** → `cache.handleDispatch` → `commands.handleMessage` (MESSAGE_CREATE).

### Caminho quente
`THREAD_CREATE` (`newly_created`, `parent_id === channelId`) → registra pendente. `MESSAGE_CREATE` do criador no tópico → `humanized` → `responder.send()` → `rest.sendMessage()` (POST `/api/v9/channels/{threadId}/messages`), síncrono quando `HUMANIZED_DELAY=0`. Logs/eventos só depois de disparar o envio.
- Pool undici com 10 conexões keep-alive; `warmup()` no READY + ping a cada 30s.
- Body sempre consumido (`dump()`/`json()`) para devolver a conexão ao pool.
- 429 → espera `retry_after` e tenta **1x**.

### `HumanizedWaiter` (único modo de envio)
- `onThreadCreate`: se o criador já postou no tópico (buffer `recent`, TTL 15s — msg pode chegar antes do THREAD_CREATE) → envia. Senão → `pending` com timeout `waitForMessage`.
- `onMessageCreate`: tópico pendente + autor = criador → cancela timeout, envia (após `humanizedDelay`, se > 0). Tópico desconhecido → grava no buffer.
- Timeout → envia **sem** `humanizedDelay` (comportamento mantido; candidato a retrabalho).
- `cancelAll()` → chamado quando `enabled` vira `false` ou o canal muda.
- Emite `pending`, `owner-message`, `timeout`, `cancelled` (para logs e futura UI).
- Não conhece gateway/REST: recebe `send` injetado. Testável com fake timers.

### Garantias do `off`
1. `config.set('enabled', false)` → listener em `index.js` chama `humanized.cancelAll()`.
2. `responder.send()` checa `config.enabled` imediatamente antes de qualquer POST.

### Dedupe
`processed: Set` no responder; entrada removida após 30s ou em falha de envio.

### Gateway (`discord/gateway.js`)
- `wss://gateway.discord.gg/?v=9&encoding=json`, sem compressão, IDENTIFY de usuário (sem intents, `capabilities` 30717).
- Heartbeat com jitter; sem ACK → `terminate()` e RESUME.
- op 7 → reconecta com RESUME (close 4000). op 9 → nova sessão se `d=false`, espera 1–5s.
- Close fatais (4004, 4010–4014) → evento `fatal` → processo encerra. 4007/4009 → sessão nova.
- Backoff exponencial até 30s.
- `subscribeChannel(guildId, channelId)` envia op 37 (`threads: true`) — necessário para receber eventos em servidores grandes. Chamado no READY e ao trocar canal.

### Comandos (`!autoPing ...`)
Só da própria conta, em qualquer chat. Todos respondem **no chat e no terminal**. Mensagem do comando apagada após 2s.
`status`, `canal <ID>`, `msg <texto>`, `delay <ms>`, `on`, `off`, `listar`, `ajuda`.

### Validação de canal
`GET /channels/:id` (REST). OK + tipo em `THREAD_PARENT_TYPES` = válido. Não há checagem de permissão computada (falha de envio é logada). Erros: 10003/404, 50001/403.

## Convenções

- Código, comentários, logs e mensagens em **PT-BR**, com emojis nos logs.
- No caminho quente: **enviar primeiro, logar depois**. Nada de `await`, fetch ou validação antes do POST.
- `properties.js` é a única fonte do fingerprint; IDENTIFY e REST devem continuar consistentes.
- Mudanças de config sempre via `config.set()` (para disparar `change`).
- Nunca logar o token.

## Pendências / riscos

- Fingerprint (`client_build_number`, versão do Chrome) fica desatualizado com o tempo — atualizar periodicamente.
- Payload de READY de usuário varia com `capabilities`; `cache.js` faz parse defensivo (`guild.properties.name` / `guild.name`). Validar ao mudar capabilities.
- op 37 e recebimento de eventos em servidores grandes: validar em uso real.
- Config volátil; multi-conta = multi-container.
- Dockerfile não testado localmente (Docker não instalado na máquina de dev).

## Roadmap

- **Mini app (UI)** para configurar/controlar instâncias. Pontos de integração prontos: `Config` (evento `change`, `snapshot()`), eventos do `HumanizedWaiter`, callback `onSent` do responder. Manter o caminho de envio isolado e intocado em latência; considerar persistência da config.
- Retrabalho do modo humanizado.
