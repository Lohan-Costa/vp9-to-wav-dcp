import { initConverter } from './converter.js';
import { initValidator } from './validator.js';

const { invoke } = window.__TAURI__.core;
const { listen }  = window.__TAURI__.event;

// ── Tab switching ─────────────────────────────────────────────────────────────
const tabConverter = document.getElementById('tab-converter');
const tabValidator = document.getElementById('tab-validator');
const panelConverter = document.getElementById('panel-converter');
const panelValidator  = document.getElementById('panel-validator');

function activateTab(tab) {
  tabConverter.classList.toggle('active', tab === 'converter');
  tabValidator.classList.toggle('active', tab === 'validator');
  tabConverter.setAttribute('aria-selected', tab === 'converter');
  tabValidator.setAttribute('aria-selected', tab === 'validator');
  panelConverter.classList.toggle('hidden', tab !== 'converter');
  panelValidator.classList.toggle('hidden', tab !== 'validator');
}

tabConverter.addEventListener('click', () => activateTab('converter'));
tabValidator.addEventListener('click', () => activateTab('validator'));

// ── Footer link ───────────────────────────────────────────────────────────────
document.getElementById('link-lohan').addEventListener('click', async (e) => {
  e.preventDefault();
  await invoke('open_url', { url: 'https://www.linkedin.com/in/lohan-costa/' });
});

// ── Init modules ─────────────────────────────────────────────────────────────
initConverter({ invoke, listen });
initValidator({ invoke, listen });
