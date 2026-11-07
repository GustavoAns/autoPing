/**
 * Modo humanizado (único modo): responder somente DEPOIS da 1ª mensagem do
 * criador do tópico, o mais rápido possível. O Discord cria o tópico e só
 * depois posta a msg inicial; responder no THREAD_CREATE faria nossa msg
 * aparecer ANTES da do criador (suspeito).
 *
 * Módulo isolado: não conhece gateway nem REST. Recebe eventos já
 * normalizados e chama o callback `send` injetado. Pensado para ser
 * reaproveitado/observado pela futura interface (mini app) via eventos.
 *
 * Fluxo:
 *   onThreadCreate -> se a 1ª msg do criador JÁ chegou (race do fórum, onde o
 *                     post inicial pode chegar antes do THREAD_CREATE) envia
 *                     após `humanizedDelay`; senão fica pendente até
 *                     `waitForMessage` ms.
 *   onMessageCreate -> se o tópico está pendente e o autor é o criador,
 *                     envia após `humanizedDelay`; se não está pendente,
 *                     guarda no buffer `recent` por `bufferTtlMs`.
 *   timeout        -> envia mesmo sem a msg do criador (sem humanizedDelay).
 *
 * Eventos: 'pending', 'owner-message', 'timeout', 'cancelled'
 */

const { EventEmitter } = require('node:events');

class HumanizedWaiter extends EventEmitter {
  /**
   * @param {object} opts
   * @param {() => number} opts.getWaitMs   tempo máx. aguardando a 1ª msg
   * @param {() => number} opts.getDelayMs  delay após a 1ª msg do criador
   * @param {(threadId: string, startTime: number, reason: string) => any} opts.send
   * @param {number} [opts.bufferTtlMs]     quanto tempo lembrar mensagens "adiantadas"
   * @param {number} [opts.maxBuffer]       tamanho máx. do buffer
   */
  constructor({ getWaitMs, getDelayMs, send, bufferTtlMs = 15000, maxBuffer = 5000 }) {
    super();
    this.getWaitMs = getWaitMs;
    this.getDelayMs = getDelayMs;
    this.sendFn = send;
    this.bufferTtlMs = bufferTtlMs;
    this.maxBuffer = maxBuffer;

    this.pending = new Map();   // threadId -> { ownerId, startTime, timer }
    this.scheduled = new Map(); // threadId -> timer (msg do criador detectada, aguardando humanizedDelay)
    this.recent = new Map();    // `${channelId}:${authorId}` -> timestamp

    this.sweepTimer = setInterval(() => this._sweep(), bufferTtlMs);
    this.sweepTimer.unref();
  }

  onThreadCreate({ threadId, ownerId, startTime = Date.now() }) {
    if (this.isTracking(threadId)) return;

    const key = `${threadId}:${ownerId}`;
    const seenAt = this.recent.get(key);
    if (seenAt !== undefined && Date.now() - seenAt <= this.bufferTtlMs) {
      this.recent.delete(key);
      this._schedule(threadId, startTime, 'owner-message-early');
      this.emit('owner-message', { threadId, ownerId, early: true });
      return;
    }

    const waitMs = this.getWaitMs();
    const timer = setTimeout(() => {
      if (!this.pending.has(threadId)) return;
      this.pending.delete(threadId);
      this.emit('timeout', { threadId, waitMs });
      this.sendFn(threadId, startTime, 'timeout');
    }, waitMs);

    this.pending.set(threadId, { ownerId, startTime, timer });
    this.emit('pending', { threadId, ownerId, waitMs });
  }

  /**
   * @returns {boolean} true se a mensagem disparou um envio
   */
  onMessageCreate({ channelId, authorId }) {
    if (!channelId || !authorId) return false;

    const p = this.pending.get(channelId);
    if (p) {
      if (authorId !== p.ownerId) return false;
      clearTimeout(p.timer);
      this.pending.delete(channelId);
      this._schedule(channelId, p.startTime, 'owner-message');
      this.emit('owner-message', { threadId: channelId, ownerId: authorId, early: false });
      return true;
    }

    // Tópico ainda desconhecido: guardar para o caso do THREAD_CREATE chegar depois.
    if (this.recent.size >= this.maxBuffer) this._sweep(true);
    this.recent.set(`${channelId}:${authorId}`, Date.now());
    return false;
  }

  isTracking(threadId) {
    return this.pending.has(threadId) || this.scheduled.has(threadId);
  }

  /**
   * Cancela tudo que está pendente/agendado (usado no `off` e troca de canal).
   * @returns {number} quantidade de tópicos cancelados
   */
  cancelAll() {
    const count = this.pending.size + this.scheduled.size;
    for (const p of this.pending.values()) clearTimeout(p.timer);
    for (const t of this.scheduled.values()) clearTimeout(t);
    this.pending.clear();
    this.scheduled.clear();
    this.recent.clear();
    if (count > 0) this.emit('cancelled', { count });
    return count;
  }

  destroy() {
    this.cancelAll();
    clearInterval(this.sweepTimer);
  }

  _schedule(threadId, startTime, reason) {
    const delayMs = this.getDelayMs();
    if (delayMs <= 0) {
      // Caminho quente: envia na hora, sem passar pelo event loop.
      this.sendFn(threadId, startTime, reason);
      return;
    }
    const timer = setTimeout(() => {
      this.scheduled.delete(threadId);
      this.sendFn(threadId, startTime, reason);
    }, delayMs);
    this.scheduled.set(threadId, timer);
  }

  _sweep(force = false) {
    const limit = Date.now() - this.bufferTtlMs;
    for (const [key, ts] of this.recent) {
      if (ts < limit) this.recent.delete(key);
    }
    // Se ainda estiver cheio, descarta os mais antigos (ordem de inserção).
    if (force) {
      for (const key of this.recent.keys()) {
        if (this.recent.size < this.maxBuffer) break;
        this.recent.delete(key);
      }
    }
  }
}

module.exports = { HumanizedWaiter };
