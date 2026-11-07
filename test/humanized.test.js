const { test, mock } = require('node:test');
const assert = require('node:assert/strict');
const { HumanizedWaiter } = require('../src/core/humanized');
const { createResponder } = require('../src/core/responder');
const { Config } = require('../src/config');

function setup({ waitMs = 5000, delayMs = 900 } = {}) {
  const sent = [];
  const waiter = new HumanizedWaiter({
    getWaitMs: () => waitMs,
    getDelayMs: () => delayMs,
    send: (threadId, startTime, reason) => sent.push({ threadId, reason }),
  });
  return { waiter, sent };
}

test('envia após msg do criador + humanizedDelay', (t) => {
  t.mock.timers.enable({ apis: ['setTimeout', 'setInterval', 'Date'] });
  const { waiter, sent } = setup();
  waiter.onThreadCreate({ threadId: 'T1', ownerId: 'U1' });
  assert.equal(waiter.onMessageCreate({ channelId: 'T1', authorId: 'OUTRO' }), false);
  assert.equal(waiter.onMessageCreate({ channelId: 'T1', authorId: 'U1' }), true);
  t.mock.timers.tick(899);
  assert.equal(sent.length, 0);
  t.mock.timers.tick(1);
  assert.deepEqual(sent, [{ threadId: 'T1', reason: 'owner-message' }]);
  t.mock.timers.tick(10000);
  assert.equal(sent.length, 1, 'timeout não deve disparar depois');
  waiter.destroy();
});

test('race do fórum: msg do criador antes do THREAD_CREATE', (t) => {
  t.mock.timers.enable({ apis: ['setTimeout', 'setInterval', 'Date'] });
  const { waiter, sent } = setup();
  waiter.onMessageCreate({ channelId: 'T2', authorId: 'U2' });
  waiter.onThreadCreate({ threadId: 'T2', ownerId: 'U2' });
  assert.equal(waiter.pending.has('T2'), false);
  t.mock.timers.tick(900);
  assert.deepEqual(sent, [{ threadId: 'T2', reason: 'owner-message-early' }]);
  waiter.destroy();
});

test('timeout envia sem msg do criador', (t) => {
  t.mock.timers.enable({ apis: ['setTimeout', 'setInterval', 'Date'] });
  const { waiter, sent } = setup();
  waiter.onThreadCreate({ threadId: 'T3', ownerId: 'U3' });
  t.mock.timers.tick(5000);
  assert.deepEqual(sent, [{ threadId: 'T3', reason: 'timeout' }]);
  waiter.destroy();
});

test('cancelAll cancela pendentes e agendados', (t) => {
  t.mock.timers.enable({ apis: ['setTimeout', 'setInterval', 'Date'] });
  const { waiter, sent } = setup();
  waiter.onThreadCreate({ threadId: 'A', ownerId: 'U' });
  waiter.onThreadCreate({ threadId: 'B', ownerId: 'U' });
  waiter.onMessageCreate({ channelId: 'B', authorId: 'U' }); // B agendado
  assert.equal(waiter.cancelAll(), 2);
  t.mock.timers.tick(60000);
  assert.equal(sent.length, 0);
  waiter.destroy();
});

test('buffer expira após TTL', (t) => {
  t.mock.timers.enable({ apis: ['setTimeout', 'setInterval', 'Date'] });
  const { waiter, sent } = setup();
  waiter.onMessageCreate({ channelId: 'T4', authorId: 'U4' });
  t.mock.timers.tick(16000);
  waiter.onThreadCreate({ threadId: 'T4', ownerId: 'U4' });
  assert.equal(waiter.pending.has('T4'), true);
  waiter.destroy();
  assert.equal(sent.length, 0);
});

test('responder: off bloqueia envio pendente', async () => {
  const config = new Config({ CHANNEL_ID: 'C1', AUTO_MESSAGE: 'oi' });
  const calls = [];
  const rest = { sendMessage: async (id, content) => calls.push({ id, content }) };
  const responder = createResponder({ config, rest });

  config.set('enabled', false);
  const ok = await responder.send('T5', Date.now(), 'timeout');
  assert.equal(ok, false);
  assert.equal(calls.length, 0);

  config.set('enabled', true);
  assert.equal(await responder.send('T5', Date.now(), 'imediato'), true);
  assert.equal(await responder.send('T5', Date.now(), 'imediato'), false, 'dedupe');
  assert.deepEqual(calls, [{ id: 'T5', content: 'oi' }]);
});

test('responder: nunca envia no THREAD_CREATE, só após msg do criador', async (t) => {
  t.mock.timers.enable({ apis: ['setTimeout', 'setInterval', 'Date'] });
  const config = new Config({ CHANNEL_ID: 'C1', AUTO_MESSAGE: 'oi', HUMANIZED_DELAY: '0' });
  const calls = [];
  const rest = { sendMessage: async (id) => calls.push(id) };
  const responder = createResponder({ config, rest });
  const waiter = new HumanizedWaiter({
    getWaitMs: () => config.waitForMessage,
    getDelayMs: () => config.humanizedDelay,
    send: responder.send,
  });
  responder.setHumanized(waiter);

  responder.handleDispatch('THREAD_CREATE', { id: 'X', parent_id: 'OUTRO', newly_created: true, owner_id: 'U' });
  responder.handleDispatch('THREAD_CREATE', { id: 'Y', parent_id: 'C1', newly_created: false, owner_id: 'U' });
  responder.handleDispatch('THREAD_CREATE', { id: 'Z', parent_id: 'C1', newly_created: true, owner_id: 'U', name: 'vaga' });
  assert.deepEqual(calls, [], 'nada enviado no THREAD_CREATE');

  responder.handleDispatch('MESSAGE_CREATE', { channel_id: 'Z', author: { id: 'OUTRO' } });
  assert.deepEqual(calls, []);
  responder.handleDispatch('MESSAGE_CREATE', { channel_id: 'Z', author: { id: 'U' } });
  assert.deepEqual(calls, ['Z'], 'com delay 0 envia de forma síncrona');
  waiter.destroy();
});

test('config: WAIT_FOR_MESSAGE 0/vazio vira 5000; HUMANIZED_DELAY padrão 900', () => {
  assert.equal(new Config({}).waitForMessage, 5000);
  assert.equal(new Config({ WAIT_FOR_MESSAGE: '0' }).waitForMessage, 5000);
  assert.equal(new Config({}).humanizedDelay, 900);
  assert.equal(new Config({ HUMANIZED_DELAY: '0' }).humanizedDelay, 0);
});

test('sleep: após responder desliga, cancela pendentes e só volta com on', async (t) => {
  t.mock.timers.enable({ apis: ['setTimeout', 'setInterval', 'Date'] });
  const config = new Config({ CHANNEL_ID: 'C1', AUTO_MESSAGE: 'oi', HUMANIZED_DELAY: '0', SLEEP_AFTER_REPLY: 'true' });
  const calls = [];
  const rest = { sendMessage: async (id) => calls.push(id) };
  const responder = createResponder({ config, rest });
  const waiter = new HumanizedWaiter({
    getWaitMs: () => config.waitForMessage,
    getDelayMs: () => config.humanizedDelay,
    send: responder.send,
  });
  responder.setHumanized(waiter);
  config.on('change', (key, value) => { if (key === 'enabled' && !value) waiter.cancelAll(); });

  responder.handleDispatch('THREAD_CREATE', { id: 'A', parent_id: 'C1', newly_created: true, owner_id: 'U' });
  responder.handleDispatch('THREAD_CREATE', { id: 'B', parent_id: 'C1', newly_created: true, owner_id: 'U' });
  responder.handleDispatch('MESSAGE_CREATE', { channel_id: 'A', author: { id: 'U' } });
  await Promise.resolve();
  assert.equal(config.enabled, false);
  assert.equal(waiter.pending.has('B'), false, 'B cancelado');

  t.mock.timers.tick(10000);
  responder.handleDispatch('THREAD_CREATE', { id: 'C', parent_id: 'C1', newly_created: true, owner_id: 'U' });
  responder.handleDispatch('MESSAGE_CREATE', { channel_id: 'C', author: { id: 'U' } });
  assert.deepEqual(calls, ['A']);

  config.set('enabled', true);
  responder.handleDispatch('THREAD_CREATE', { id: 'D', parent_id: 'C1', newly_created: true, owner_id: 'U' });
  responder.handleDispatch('MESSAGE_CREATE', { channel_id: 'D', author: { id: 'U' } });
  assert.deepEqual(calls, ['A', 'D']);
  waiter.destroy();
});

test('sleep: envio falhou religa', async () => {
  const config = new Config({ CHANNEL_ID: 'C1', AUTO_MESSAGE: 'oi', SLEEP_AFTER_REPLY: 'true' });
  const rest = { sendMessage: async () => { throw new Error('HTTP 500'); } };
  const responder = createResponder({ config, rest });
  assert.equal(await responder.send('T', Date.now(), 'x'), false);
  assert.equal(config.enabled, true);
});

mock.reset();
