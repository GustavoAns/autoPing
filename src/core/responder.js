/**
 * Responder: decide QUANDO enviar a mensagem automática.
 *
 * Regra: a resposta deve aparecer SEMPRE depois da 1ª mensagem do criador do
 * tópico (que o Discord posta logo após criar o tópico). Por isso o gatilho de
 * envio é a 1ª mensagem do criador, não o THREAD_CREATE:
 *
 * - THREAD_CREATE no canal monitorado -> registra o tópico no HumanizedWaiter.
 * - MESSAGE_CREATE -> repassado ao HumanizedWaiter, que dispara o envio quando
 *   for a msg do criador (ou se ela chegou antes do THREAD_CREATE).
 * - Sem msg do criador em `waitForMessage` ms -> envia mesmo assim (timeout).
 *
 * Toda tentativa de envio passa por `send()`, que checa `config.enabled`
 * imediatamente antes do POST (garante que `off` bloqueia qualquer envio).
 *
 * Sleep (`config.sleepAfterReply`): após disparar o POST, desliga o AutoPing
 * (enabled=false -> cancela pendentes) até `!autoPing on`. Se o envio
 * falhar, religa.
 */

const DEDUPE_TTL_MS = 30000;

function createResponder({ config, rest, onSent = () => {} }) {
  const processed = new Set();
  let humanized = null;

  function setHumanized(waiter) {
    humanized = waiter;
  }

  async function send(threadId, startTime, reason) {
    if (!config.enabled || !config.autoMessage) {
      console.log(`   ⏹️ Envio cancelado para ${threadId} (AutoPing desativado ou sem mensagem)`);
      return false;
    }
    if (processed.has(threadId)) return false;
    processed.add(threadId);

    const sendStart = Date.now();
    const sending = rest.sendMessage(threadId, config.autoMessage);

    // Sleep: desliga logo após disparar o POST (cancela pendentes e bloqueia
    // envios concorrentes). Volta só com `!autoPing on`.
    const sleeping = config.sleepAfterReply;
    if (sleeping) config.set('enabled', false);

    try {
      await sending;
      const now = Date.now();
      const responseTime = now - startTime;
      console.log(`   ✅ Mensagem enviada (${reason}) no tópico ${threadId}`);
      console.log(`   ⚡ Tempo total desde o tópico: ${responseTime}ms | POST: ${now - sendStart}ms\n`);
      setTimeout(() => processed.delete(threadId), DEDUPE_TTL_MS).unref();
      if (sleeping) console.log('   💤 Sleep: AutoPing desativado após responder. Use !autoPing on para voltar.\n');
      onSent({ threadId, responseTime, reason, sleeping });
      return true;
    } catch (error) {
      processed.delete(threadId);
      if (sleeping && !config.enabled) {
        // Envio falhou: religa para não perder a próxima vaga.
        config.set('enabled', true);
        console.log('   🔁 Sleep revertido (envio falhou): AutoPing reativado.');
      }
      console.error(`   ❌ Erro ao enviar no tópico ${threadId}: ${error.message}\n`);
      return false;
    }
  }

  function handleDispatch(t, d) {
    if (!humanized) return;

    if (t === 'MESSAGE_CREATE') {
      if (!config.enabled) return;
      humanized.onMessageCreate({ channelId: d.channel_id, authorId: d.author && d.author.id });
      return;
    }

    if (t === 'THREAD_CREATE') {
      if (!config.enabled || !config.autoMessage) return;
      if (!d.newly_created || d.parent_id !== config.channelId) return;
      if (processed.has(d.id)) return;

      humanized.onThreadCreate({ threadId: d.id, ownerId: d.owner_id, startTime: Date.now() });
      console.log(`\n⚡ Novo tópico detectado: "${d.name}" (${d.id}) | criador ${d.owner_id}`);
      if (humanized.pending.has(d.id)) {
        console.log(`   ⏳ Aguardando 1ª mensagem do criador (até ${config.waitForMessage}ms)...`);
      }
    }
  }

  return { handleDispatch, send, setHumanized };
}

module.exports = { createResponder };
