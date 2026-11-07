# ===========================================
# Discord AutoPing - Dockerfile
# ===========================================

# Usar Node.js 24 LTS (Alpine para imagem menor)
FROM node:24-alpine

# Metadados
LABEL maintainer="AutoPing"
LABEL description="Discord AutoPing para threads"

# Criar diretório de trabalho
WORKDIR /app

# Copiar arquivos de dependências primeiro (melhor cache)
COPY package*.json ./

# Instalar dependências
RUN npm ci --omit=dev && \
    npm cache clean --force

# Copiar código fonte
COPY src/ ./src/

# Não rodar como root por segurança
RUN addgroup -g 1001 -S nodejs && \
    adduser -S autorespond -u 1001
USER autorespond

# Comando padrão
CMD ["node", "src/index.js"]
