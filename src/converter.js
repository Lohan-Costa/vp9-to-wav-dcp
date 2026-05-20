const { save } = window.__TAURI__.dialog;

export function initConverter({ invoke, listen }) {
  // ── DOM refs ───────────────────────────────────────────────────────────────
  const dropZone     = document.getElementById('drop-zone');
  const fileInput    = document.getElementById('file-input');
  const btnPickFile  = document.getElementById('btn-pick-file');

  const stateDrop      = document.getElementById('state-drop');
  const stateAnalysis  = document.getElementById('state-analysis');
  const stateProcessing= document.getElementById('state-processing');
  const stateDone      = document.getElementById('state-done');
  const stateError     = document.getElementById('state-error');

  const attrList       = document.getElementById('attr-list');
  const cropConfirm    = document.getElementById('crop-confirm');
  const cropDescription= document.getElementById('crop-description');
  const btnConfirmCrop = document.getElementById('btn-confirm-crop');
  const btnCancelCrop  = document.getElementById('btn-cancel-crop');
  const analysisActions= document.getElementById('analysis-actions');
  const btnStartConvert= document.getElementById('btn-start-convert');
  const btnPickAnother = document.getElementById('btn-pick-another-1');

  const barEncoding  = document.getElementById('bar-encoding');
  const barPackaging = document.getElementById('bar-packaging');
  const pctEncoding  = document.getElementById('pct-encoding');
  const pctPackaging = document.getElementById('pct-packaging');
  const progressMsg  = document.getElementById('progress-message');
  const btnCancel    = document.getElementById('btn-cancel');
  const consoleOutput= document.getElementById('console-output');

  const btnSaveWav      = document.getElementById('btn-save-wav');
  const btnConvertAnother= document.getElementById('btn-convert-another');

  const errorTitle  = document.getElementById('error-title');
  const errorMsg    = document.getElementById('error-msg');
  const btnCopyLog  = document.getElementById('btn-copy-log');
  const btnRetry    = document.getElementById('btn-retry');

  // ── State ──────────────────────────────────────────────────────────────────
  let currentFilePath = null;
  let currentVideoInfo = null;
  let cropSelected = false;
  let progressUnlisten = null;
  let logUnlisten = null;
  let lastOriginalName = '';

  // ── Helpers ────────────────────────────────────────────────────────────────
  function showState(name) {
    [stateDrop, stateAnalysis, stateProcessing, stateDone, stateError]
      .forEach(s => s.classList.add('hidden'));
    document.getElementById('state-' + name)?.classList.remove('hidden');
  }

  function appendLog(msg) {
    consoleOutput.textContent += msg + '\n';
    consoleOutput.scrollTop = consoleOutput.scrollHeight;
  }

  function setProgress(stage, pct, msg) {
    if (stage === 'encoding') {
      barEncoding.style.width  = pct + '%';
      pctEncoding.textContent  = Math.round(pct) + '%';
    } else if (stage === 'packaging') {
      barPackaging.style.width  = pct + '%';
      pctPackaging.textContent  = Math.round(pct) + '%';
    }
    progressMsg.textContent = msg || '';
  }

  function resetProgress() {
    barEncoding.style.width   = '0%';
    barPackaging.style.width  = '0%';
    pctEncoding.textContent   = '0%';
    pctPackaging.textContent  = '0%';
    progressMsg.textContent   = '';
    consoleOutput.textContent = '';
  }

  function showError(title, msg) {
    errorTitle.textContent = title;
    errorMsg.textContent   = msg;
    showState('error');
  }

  // ── Attribute status labels ────────────────────────────────────────────────
  const BADGE = {
    conformant:    { symbol: '✓', cls: 'badge-ok',      word: 'mantido'  },
    will_adjust:   { symbol: '↻', cls: 'badge-adjust',  word: 'ajustado' },
    needs_confirm: { symbol: '⚠', cls: 'badge-confirm', word: ''         },
  };

  function renderAttrs(info) {
    attrList.innerHTML = '';
    const attrs = [
      { key: 'codec',        label: 'Codec'         },
      { key: 'frame_rate',   label: 'Frame rate'    },
      { key: 'bitrate',      label: 'Bitrate'       },
      { key: 'pixel_format', label: 'Pixel format'  },
      { key: 'resolution',   label: 'Resolução'     },
    ];

    attrs.forEach(({ key, label }) => {
      const attr  = info[key];
      const badge = BADGE[attr.status];
      const row   = document.createElement('div');
      row.className = 'attr-row';

      const labelSpan = document.createElement('span');
      labelSpan.className = 'attr-label';
      labelSpan.textContent = label;

      const valSpan = document.createElement('span');
      valSpan.className = 'attr-value';

      const sym = document.createElement('span');
      sym.className = `attr-badge ${badge.cls}`;
      sym.textContent = badge.symbol;

      let text = attr.current;
      if (attr.target) text += ` → ${attr.target}`;
      if (badge.word)  text += ` (${badge.word})`;

      valSpan.appendChild(sym);
      valSpan.appendChild(document.createTextNode(text));

      row.appendChild(labelSpan);
      row.appendChild(valSpan);
      attrList.appendChild(row);
    });
  }

  // ── File picked ────────────────────────────────────────────────────────────
  async function handleFile(filePath, originalName) {
    currentFilePath  = filePath;
    lastOriginalName = originalName.replace(/\.[^.]+$/, '');
    cropSelected     = false;

    showState('analysis');
    attrList.innerHTML = '<div style="color:var(--text-secondary);font-size:13px">Analisando…</div>';
    cropConfirm.classList.add('hidden');
    analysisActions.classList.remove('hidden');
    btnStartConvert.disabled = false;

    try {
      currentVideoInfo = await invoke('analyze_video', { path: filePath });
      renderAttrs(currentVideoInfo);

      if (currentVideoInfo.needs_crop) {
        cropDescription.textContent = currentVideoInfo.crop_description || '';
        cropConfirm.classList.remove('hidden');
        // Hide start button until user decides
        btnStartConvert.disabled = true;
      }
    } catch (err) {
      showError(
        'Não foi possível ler o vídeo',
        'O arquivo pode estar corrompido, protegido ou em um formato não suportado. Tente com outro arquivo.'
      );
    }
  }

  // ── Drag & drop ────────────────────────────────────────────────────────────
  dropZone.addEventListener('dragover', e => {
    e.preventDefault();
    dropZone.classList.add('drag-over');
  });
  dropZone.addEventListener('dragleave', () => dropZone.classList.remove('drag-over'));
  dropZone.addEventListener('drop', e => {
    e.preventDefault();
    dropZone.classList.remove('drag-over');
    const file = e.dataTransfer?.files?.[0];
    if (file) handleFile(file.path, file.name);
  });
  dropZone.addEventListener('keydown', e => {
    if (e.key === 'Enter' || e.key === ' ') fileInput.click();
  });

  btnPickFile.addEventListener('click', e => { e.stopPropagation(); fileInput.click(); });
  dropZone.addEventListener('click', () => fileInput.click());
  fileInput.addEventListener('change', () => {
    const file = fileInput.files?.[0];
    if (file) handleFile(file.path, file.name);
    fileInput.value = '';
  });

  // ── Crop decision ──────────────────────────────────────────────────────────
  btnConfirmCrop.addEventListener('click', () => {
    cropSelected = true;
    cropConfirm.classList.add('hidden');
    btnStartConvert.disabled = false;
  });
  btnCancelCrop.addEventListener('click', () => {
    cropConfirm.classList.add('hidden');
    showState('drop');
  });

  // ── Start conversion ───────────────────────────────────────────────────────
  btnStartConvert.addEventListener('click', startConversion);
  btnPickAnother.addEventListener('click', () => showState('drop'));

  async function startConversion() {
    if (!currentFilePath) return;

    resetProgress();
    showState('processing');

    // Listen for progress events
    progressUnlisten = await listen('conversion-progress', ({ payload }) => {
      setProgress(payload.stage, payload.percent, payload.message);
    });
    logUnlisten = await listen('log-line', ({ payload }) => {
      appendLog(`[${payload.level.toUpperCase()}] ${payload.message}`);
    });

    try {
      await invoke('start_conversion', {
        path: currentFilePath,
        crop: cropSelected || (currentVideoInfo?.needs_crop ?? false),
      });

      progressUnlisten?.();
      logUnlisten?.();
      setProgress('encoding', 100, '');
      setProgress('packaging', 100, '');
      showState('done');
    } catch (err) {
      progressUnlisten?.();
      logUnlisten?.();
      if (err.includes('Cancelado')) {
        showState('drop');
      } else {
        showError('Erro durante a conversão', friendlyError(err));
      }
    }
  }

  // ── Cancel ─────────────────────────────────────────────────────────────────
  btnCancel.addEventListener('click', async () => {
    await invoke('cancel_conversion');
  });

  // ── Save WAV ───────────────────────────────────────────────────────────────
  btnSaveWav.addEventListener('click', async () => {
    const suggestedName = lastOriginalName
      ? `${lastOriginalName}_ch15.wav`
      : 'libras_ch15.wav';

    const destPath = await save({
      defaultPath: suggestedName,
      filters: [{ name: 'Arquivo WAV', extensions: ['wav'] }],
    });

    if (!destPath) return;

    try {
      await invoke('save_wav', { destPath });
    } catch (err) {
      showError('Erro ao salvar o arquivo', friendlyError(err));
    }
  });

  btnConvertAnother.addEventListener('click', () => {
    currentFilePath  = null;
    currentVideoInfo = null;
    showState('drop');
  });

  // ── Error actions ──────────────────────────────────────────────────────────
  btnCopyLog.addEventListener('click', async () => {
    const log = await invoke('get_log');
    await navigator.clipboard.writeText(log);
  });

  btnRetry.addEventListener('click', () => {
    if (currentFilePath) {
      showState('analysis');
    } else {
      showState('drop');
    }
  });

  // ── Friendly error messages ────────────────────────────────────────────────
  function friendlyError(raw) {
    if (raw.includes('Cancelado'))         return 'A operação foi cancelada.';
    if (raw.includes('muito curto'))       return raw;
    if (raw.includes('grande demais'))     return 'Um ou mais chunks VP9 ficaram acima do limite. Tente usar um vídeo com menos detalhes ou bitrate menor.';
    if (raw.includes('corrompido'))        return raw;
    if (raw.includes('não suportado'))     return raw;
    if (raw.includes('permissão') || raw.includes('Permission'))
      return 'Sem permissão para gravar no destino escolhido. Tente salvar em outro local.';
    return 'Ocorreu um erro inesperado. Abra o log técnico para mais detalhes.';
  }
}
