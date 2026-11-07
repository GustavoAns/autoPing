/**
 * Identidade do cliente (fingerprint) usada tanto no IDENTIFY do gateway quanto
 * nos headers REST (User-Agent / X-Super-Properties). Manter os dois
 * consistentes evita parecer um cliente "estranho" para o Discord.
 *
 * CLIENT_BUILD_NUMBER pode ser sobrescrito via .env quando o Discord atualizar.
 */

const BROWSER_VERSION = '140.0.0.0';
const USER_AGENT = `Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/${BROWSER_VERSION} Safari/537.36`;
const CLIENT_BUILD_NUMBER = parseInt(process.env.CLIENT_BUILD_NUMBER, 10) || 440000;
const LOCALE = 'pt-BR';

const PROPERTIES = Object.freeze({
  os: 'Windows',
  browser: 'Chrome',
  device: '',
  system_locale: LOCALE,
  has_client_mods: false,
  browser_user_agent: USER_AGENT,
  browser_version: BROWSER_VERSION,
  os_version: '10',
  referrer: '',
  referring_domain: '',
  referrer_current: '',
  referring_domain_current: '',
  release_channel: 'stable',
  client_build_number: CLIENT_BUILD_NUMBER,
  client_event_source: null,
});

const SUPER_PROPERTIES = Buffer.from(JSON.stringify(PROPERTIES)).toString('base64');

// Mesmo valor usado pelo cliente web oficial.
const CAPABILITIES = 30717;

const API_VERSION = 9;

module.exports = {
  PROPERTIES,
  SUPER_PROPERTIES,
  USER_AGENT,
  LOCALE,
  CAPABILITIES,
  API_VERSION,
};
