# 🟢 AutoPing

App leve para Windows que monitora um canal do Discord e responde automaticamente quando um novo tópico é criado, sempre logo após a primeira mensagem do criador.

> ⚠️ **AVISO**: o AutoPing usa a sua conta de usuário (self-bot), o que viola os Termos de Serviço do Discord e pode resultar em banimento. Use por sua conta e risco.

---

## ✨ Funcionalidades

- Resposta automática em novos tópicos do canal escolhido, **depois** da mensagem inicial do criador
- **Modo humano**: espera extra configurável após a mensagem do criador
- **Sleep após responder**: desliga sozinho depois de pegar uma vaga
- **Som ao enviar** e **modo alarme** (toca até você parar)
- Ícone na bandeja que muda de cor: 🟢 ligado · 🔴 desligado · 🔵 sleep (pisca 🟠 com alarme)
- Configurações aplicadas na hora, sem reiniciar
- Muito leve: oculto na bandeja usa ~12MB de RAM e CPU praticamente zero
- Token guardado no **Cofre do Windows** (criptografado)

---

## 🚀 Instalação

1. Baixe e execute `AutoPing_x.y.z_x64-setup.exe`.
2. Na primeira abertura, a tela de configurações aparece:
   - **Token**: o token da sua conta (veja abaixo)
   - **Canal**: escolha na lista ou cole o ID
   - **Mensagem**: o texto que será enviado
3. Clique em **Salvar**. Pronto: o AutoPing fica na bandeja (perto do relógio).

> Já usava a versão de console? Se houver um `.env` na pasta, o app importa token, canal e mensagem automaticamente na primeira execução.

### Obter o token

1. Abra o Discord **no navegador** e pressione `F12`.
2. Pressione `Ctrl+Shift+M` (modo de emulação mobile).
3. Aba **Application** → **Local Storage** → `https://discord.com` → copie o valor de `token` (sem aspas).

**Nunca compartilhe seu token.**

---

## 🖱️ Uso

| Ação | Como |
|---|---|
| Abrir / ocultar | Clique no ícone da bandeja |
| Ligar / desligar | Botão na janela ou menu da bandeja |
| Acordar do sleep | Botão **Acordar** (azul) |
| Parar alarme | Botão **Parar alarme**, clique no ícone ou menu da bandeja |
| Configurações | Engrenagem ⚙ na janela |
| Sair | Menu da bandeja → **Sair** |

### Configurações

| Opção | Descrição |
|---|---|
| Token | Salvo no Cofre do Windows; deixe vazio para manter |
| Canal | Canal (texto, fórum, mídia ou anúncios) onde os tópicos são criados |
| Mensagem | Texto enviado (até 2000 caracteres) |
| Aguardar 1ª msg do criador | Tempo máximo esperando a mensagem do criador; depois envia mesmo assim |
| Modo humano | Espera extra (ms) após a mensagem do criador. Desligado = mais rápido |
| Sleep após responder | Depois de responder, desliga até você acordar |
| Som ao enviar | Toca um aviso curto |
| Modo alarme | Toca sem parar até você desligar |

---

## 🛠️ Desenvolvimento

Requisitos: Windows, [Rust](https://rustup.rs) (MSVC), VS 2022 Build Tools (C++), Node.js 22+.

```bash
npm install
npm run dev     # app em modo desenvolvimento
npm test        # testes do core (Rust)
npm run build   # gera o .exe e o instalador em src-tauri/target/release/
```

Arquivos do app: `%APPDATA%\com.autoping.desktop\` (`config.json` e `logs/`).

---

## 📜 Licença

Uso pessoal e educacional. Use com responsabilidade.
