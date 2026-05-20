const { save } = window.__TAURI__.dialog;
const { convertFileSrc } = window.__TAURI__.core;

export function initValidator({ invoke, listen }) {
  // ── DOM refs ───────────────────────────────────────────────────────────────
  const vdropZone   = document.getElementById('vdrop-zone');
  const wavInput    = document.getElementById('wav-input');
  const btnPickWav  = document.getElementById('btn-pick-wav');

  const vstateDrop    = document.getElementById('vstate-drop');
  const vstateRunning = document.getElementById('vstate-running');
  const vstateResults = document.getElementById('vstate-results');
  const vstateError   = document.getElementById('vstate-error');

  const checkList        = document.getElementById('check-list');
  const videoPlayerWrap  = document.getElementById('video-player-wrap');
  const videoMeta        = document.getElementById('video-meta');
  const videoPlayer      = document.getElementById('video-player');
  const btnSaveVideo     = document.getElementById('btn-save-video');
  const btnCopyReport    = document.getElementById('btn-copy-report');
  const btnValidateAnother   = document.getElementById('btn-validate-another');
  const btnValidateAnotherErr= document.getElementById('btn-validate-another-err');

  const verrorTitle  = document.getElementById('verror-title');
  const verrorMsg    = document.getElementById('verror-msg');
  const btnVcopyLog  = document.getElementById('btn-vcopy-log');

  // ── State ──────────────────────────────────────────────────────────────────
  let lastResult = null;

  // ── Helpers ────────────────────────────────────────────────────────────────
  function showVState(name) {
    [vstateDrop, vstateRunning, vstateResults, vstateError]
      .forEach(s => s.classList.add('hidden'));
    document.getElementById('vstate-' + name)?.classList.remove('hidden');
  }

  function showVError(title, msg) {
    verrorTitle.textContent = title;
    verrorMsg.textContent   = msg;
    showVState('error');
  }

  function renderChecks(result) {
    checkList.innerHTML = '';
    result.checks.forEach(item => {
      const row = document.createElement('div');
      row.className = 'check-row';

      const icon = document.createElement('span');
      icon.className = `check-icon ${item.ok ? 'ok' : 'fail'}`;
      icon.textContent = item.ok ? '✓' : '✗';

      const name = document.createElement('span');
      name.className = 'check-name';
      name.textContent = item.label;

      const detail = document.createElement('span');
      detail.className = 'check-detail';
      detail.textContent = item.detail;

      row.appendChild(icon);
      row.appendChild(name);
      row.appendChild(detail);
      checkList.appendChild(row);
    });
  }

  // ── File handling ──────────────────────────────────────────────────────────
  async function handleWavFile(filePath) {
    showVState('running');

    try {
      const result = await invoke('start_validation', { path: filePath });
      lastResult = result;

      renderChecks(result);

      // Video player
      if (result.video_path) {
        const assetUrl = convertFileSrc(result.video_path);
        videoPlayer.src = assetUrl;
        videoMeta.textContent = result.video_info || '';
        videoPlayerWrap.classList.remove('hidden');
        btnSaveVideo.classList.remove('hidden');
      } else {
        videoPlayerWrap.classList.add('hidden');
        btnSaveVideo.classList.add('hidden');
      }

      showVState('results');
    } catch (err) {
      showVError(
        'Erro ao validar o arquivo',
        'Não foi possível processar o WAV. Verifique se o arquivo não está corrompido e tente novamente.'
      );
    }
  }

  // ── Drag & drop ────────────────────────────────────────────────────────────
  vdropZone.addEventListener('dragover', e => {
    e.preventDefault();
    vdropZone.classList.add('drag-over');
  });
  vdropZone.addEventListener('dragleave', () => vdropZone.classList.remove('drag-over'));
  vdropZone.addEventListener('drop', e => {
    e.preventDefault();
    vdropZone.classList.remove('drag-over');
    const file = e.dataTransfer?.files?.[0];
    if (file) handleWavFile(file.path);
  });
  vdropZone.addEventListener('click', () => wavInput.click());
  vdropZone.addEventListener('keydown', e => {
    if (e.key === 'Enter' || e.key === ' ') wavInput.click();
  });

  btnPickWav.addEventListener('click', e => { e.stopPropagation(); wavInput.click(); });
  wavInput.addEventListener('change', () => {
    const file = wavInput.files?.[0];
    if (file) handleWavFile(file.path);
    wavInput.value = '';
  });

  // ── Actions ────────────────────────────────────────────────────────────────
  btnSaveVideo.addEventListener('click', async () => {
    const destPath = await save({
      defaultPath: 'libras_recuperado.webm',
      filters: [{ name: 'WebM', extensions: ['webm'] }],
    });
    if (!destPath) return;
    try {
      await invoke('save_recovered_video', { destPath });
    } catch (err) {
      alert('Erro ao salvar: ' + err);
    }
  });

  btnCopyReport.addEventListener('click', () => {
    if (!lastResult) return;
    const lines = [
      '=== Relatório de Validação ISDCF Doc 13 ===',
      '',
      ...lastResult.checks.map(c => `${c.ok ? '✓' : '✗'} ${c.label}: ${c.detail}`),
      '',
      `Total de chunks: ${lastResult.total_chunks}`,
      `Chunks íntegros: ${lastResult.good_chunks}`,
      `Duração reconstruída: ${lastResult.duration_secs.toFixed(1)} s`,
    ];
    navigator.clipboard.writeText(lines.join('\n'));
  });

  const resetValidator = () => {
    lastResult = null;
    videoPlayer.src = '';
    showVState('drop');
  };

  btnValidateAnother.addEventListener('click', resetValidator);
  btnValidateAnotherErr.addEventListener('click', resetValidator);

  btnVcopyLog.addEventListener('click', async () => {
    const log = await invoke('get_log');
    await navigator.clipboard.writeText(log);
  });
}
