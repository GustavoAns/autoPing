# 🤖 Discord AutoPing

Ferramenta que monitora um canal do Discord e responde automaticamente quando um novo tópico (thread) é criado.

> ⚠️ **AVISO**: O uso de self-bots viola os Termos de Serviço do Discord e pode resultar em banimento da sua conta. Use por sua conta e risco, porem a maior parte dos banimentos é devido span.

---

## 📋 Funcionalidades

- ✅ Monitora um canal específico por novos tópicos
- ✅ Envia mensagem automaticamente quando um tópico é criado
- ✅ Resposta ultra-rápida (milissegundos)
- ✅ Dockerizado para fácil distribuição
- ✅ Utilitário para listar IDs de canais

---

## 🚀 Instalação Rápida

### Opção 1: Com Docker (Recomendado para iniciantes)

**Pré-requisito:** Instale o [Docker Desktop](https://www.docker.com/products/docker-desktop/) (baixe, instale e abra o programa)

#### Passo a passo no Windows:

**1. Baixe o projeto** - Extraia a pasta do projeto para um local fácil, por exemplo: `C:\autoPing`

**2. Configure o arquivo `.env`:**
   - Abra a pasta do projeto
   - Crie um arquivo chamado `.env` (pode copiar do `.env.example`)
   - Abra com o Bloco de Notas e preencha:
   ```
   DISCORD_TOKEN=seu_token_aqui
   CHANNEL_ID=123456789012345678
   AUTO_MESSAGE=222/555/666-FB/666-Rep
   ```
   - Salve o arquivo

**3. Abra o CMD na pasta do projeto:**
   - Abra o Explorador de Arquivos e vá até a pasta do projeto
   - Clique na barra de endereço, digite `cmd` e pressione Enter
   - Ou: Abra o CMD e digite: `cd C:\autoPing` (substitua pelo seu caminho)

**4. Execute o comando para iniciar:**
```cmd
docker-compose up -d --build
```

**5. Pronto!** O bot está rodando. Para ver os logs:
```cmd
docker-compose logs -f
```

#### Comandos úteis (copie e cole no CMD):

| O que você quer fazer | Comando |
|----------------------|---------|
| Iniciar o bot | `docker-compose up -d` |
| Parar o bot | `docker-compose down` |
| Ver logs (o que está acontecendo) | `docker-compose logs -f` |
| Reiniciar após alterar o .env | `docker-compose restart` |
| Atualizar após mudar o código | `docker-compose up -d --build` |

> 💡 **Dica:** Pressione `Ctrl+C` para sair da visualização de logs

---

### Opção 2: Sem Docker

**Requisitos:** Node.js 20 ou superior

```bash
# 1. Instalar dependências
npm install

# 2. Configurar .env (veja seção abaixo)

# 3. Executar
npm start
```

---

## ⚙️ Configuração

### 1. Obter Token do Discord

O token é necessário para autenticar sua conta. **NUNCA compartilhe seu token!**

**Método (atualizado 2025):**
1. Abra o Discord **no navegador** (não funciona no app desktop)
2. Pressione `F12` para abrir DevTools
3. Pressione `Ctrl+Shift+M` para ativar o **modo de emulação mobile** (obrigatório)
4. Vá na aba "Console"
5. Cole e execute:

```javascript
const iframe = document.createElement('iframe');
document.body.appendChild(iframe);
const token = JSON.parse(iframe.contentWindow.localStorage.token);
console.log('Token:', token);
iframe.remove();
```

6. Copie o token exibido no console

**Método alternativo (sem código):**
1. Abra o Discord no navegador e pressione `F12`
2. Pressione `Ctrl+Shift+M` para ativar emulação mobile
3. Vá na aba `Application` > `Local Storage` > `https://discord.com`
4. Procure a chave `token` e copie o valor (sem as aspas)

### 2. Obter ID do Canal

**Método Manual:**
1. Vá em `Configurações do Discord > Avançado`
2. Ative `Modo de Desenvolvedor`
3. Clique com botão direito no canal desejado
4. Clique em `Copiar ID`

**Método Automático (este projeto):**
```bash
# Configure apenas o DISCORD_TOKEN primeiro, depois execute:
npm run list-channels
```

Isso listará todos os servidores e canais com seus IDs.

### 3. Configurar arquivo .env

Copie `.env.example` para `.env` e preencha:

```env
# Seu token do Discord
DISCORD_TOKEN=seu_token_aqui

# ID do canal a ser monitorado
CHANNEL_ID=123456789012345678

# Mensagem a ser enviada
AUTO_MESSAGE=222/555/666-FB/666-Rep

# Tempo de espera pela 1ª mensagem (em ms)
# 0 = responde imediatamente | 5000 = aguarda até 5 segundos
WAIT_FOR_MESSAGE=5000
```

---

## 📁 Estrutura do Projeto

```
autoPing/
├── src/
│   ├── index.js              # Código principal
│   └── utils/
│       └── listChannels.js   # Utilitário para listar canais
├── .env                      # Configurações (não commitado)
├── .env.example              # Exemplo de configurações
├── .gitignore
├── .dockerignore
├── Dockerfile
├── docker-compose.yml
├── package.json
└── README.md
```

---

## 🛠️ Comandos NPM

| Comando | Descrição |
|---------|-----------|
| `npm start` | Inicia o AutoPing |
| `npm run dev` | Inicia com auto-reload (desenvolvimento) |
| `npm run list-channels` | Lista todos os canais e seus IDs |

---

## 💬 Comandos do Bot (via Discord)

Digite estes comandos em **qualquer chat do Discord** (serão deletados automaticamente):

| Comando | Descrição |
|---------|-----------|
| `!autoPing status` | Mostra a configuração atual |
| `!autoPing canal ID` | Define o canal a ser monitorado |
| `!autoPing msg TEXTO` | Define a mensagem automática |
| `!autoPing delay MS` | Define tempo de espera (0 = imediato) |
| `!autoPing listar` | Lista todos os canais disponíveis |
| `!autoPing on` | Ativa o AutoPing |
| `!autoPing off` | Desativa o AutoPing |
| `!autoPing ajuda` | Mostra todos os comandos |

**Exemplos:**
```
!autoPing canal 123456789012345678
!autoPing msg Olá! Tenho interesse em participar!
!autoPing delay 5000
```

> ⚠️ As alterações via comandos são **temporárias**. Para torná-las permanentes, edite o arquivo `.env`.

---

## 🐳 Referência Rápida Docker

> **Lembre-se:** Todos os comandos devem ser executados na pasta do projeto (onde está o arquivo `docker-compose.yml`)

```cmd
REM Iniciar o bot
docker-compose up -d

REM Parar o bot
docker-compose down

REM Ver logs em tempo real (Ctrl+C para sair)
docker-compose logs -f

REM Reiniciar (usar após alterar .env)
docker-compose restart

REM Reconstruir (usar após alterar código)
docker-compose up -d --build

REM Ver se o container está rodando
docker ps
```

---

## 🔧 Compartilhar com Outras Pessoas

Para distribuir este projeto:

1. **Compartilhe os arquivos** (sem o `.env` e `node_modules`)
2. A pessoa deve:
   - Ter Docker instalado
   - Criar seu próprio `.env` com seu token
   - Executar `docker-compose up -d`

---

## ❓ Solução de Problemas

### "Token inválido"
- Verifique se copiou o token corretamente (com as aspas removidas)
- Gere um novo token se necessário

### "Canal não encontrado"
- Use `npm run list-channels` para verificar o ID correto
- Certifique-se de que sua conta tem acesso ao canal

### "Mensagem não enviada"
- Verifique se você tem permissão para enviar mensagens no canal
- Confira os logs para mensagens de erro específicas

---

## 📜 Licença

Este projeto é para uso pessoal e educacional. Use com responsabilidade.
