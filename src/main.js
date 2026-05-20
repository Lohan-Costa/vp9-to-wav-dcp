import { initConverter } from './converter.js';
import { initValidator } from './validator.js';

// Lazy accessor — never destructure at module top-level
const tauri = () => window.__TAURI__;

// ── Tab state ─────────────────────────────────────────────────────────────────
let currentTab = 'converter';

const tabConverter   = document.getElementById('tab-converter');
const tabValidator   = document.getElementById('tab-validator');
const panelConverter = document.getElementById('panel-converter');
const panelValidator  = document.getElementById('panel-validator');

function activateTab(tab) {
  currentTab = tab;
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
  await tauri().core.invoke('open_url', { url: 'https://www.linkedin.com/in/lohan-costa/' });
});

// ── Init modules (return drag-drop interface) ─────────────────────────────────
const converter = initConverter(tauri);
const validator = initValidator(tauri);

function activeWidget() {
  return currentTab === 'converter' ? converter : validator;
}

// ── Global Tauri OS drag-drop events ─────────────────────────────────────────
// HTML5 dragover/drop events do NOT fire for OS-level file drops in Tauri 2.
// These Tauri events work globally for the whole window.
(async () => {
  await tauri().event.listen('tauri://drag-enter', () => {
    activeWidget().setDragOver(true);
  });
  await tauri().event.listen('tauri://drag-leave', () => {
    activeWidget().setDragOver(false);
  });
  await tauri().event.listen('tauri://drag-drop', ({ payload }) => {
    activeWidget().setDragOver(false);
    const path = payload.paths?.[0];
    if (path) activeWidget().handleDrop(path);
  });
})();
