export function initConverter(tauri) {
  // ── DOM refs ───────────────────────────────────────────────────────────────
  const dropZone      = document.getElementById('drop-zone');
  const btnPickFile   = document.getElementById('btn-pick-file');

  const stateDrop       = document.getElementById('state-drop');
  const stateAnalysis   = document.getElementById('state-analysis');
  const stateProcessing = document.getElementById('state-processing');
  const stateDone       = document.getElementById('state-done');
  const stateError      = document.getElementById('state-error');

  const attrList        = document.getElementById('attr-list');
  const cropConfirm     = document.getElementById('crop-confirm');
  const cropDescription = document.getElementById('crop-description');
  const btnConfirmCrop  = document.getElementById('btn-confirm-crop');
  const btnCancelCrop   = document.getElementById('btn-cancel-crop');
  const analysisActions = document.getElementById('analysis-actions');
  const btnStartConvert = document.getElementById('btn-start-convert');
  const btnPickAnother  = document.getElementById('btn-pick-another-1');

  const barEncoding   = document.getElementById('bar-encoding');
  const barPackaging  = document.getElementById('bar-packaging');
  const pctEncoding   = document.getElementById('pct-encoding');
  const pctPackaging  = document.getElementById('pct-packaging');
  const progressMsg   = document.getElementById('progress-message');
  const btnCancel     = document.getElementById('btn-cancel');
  const consoleOutput = document.getElementById('console-output');

  const btnSaveWav        = document.getElementById('btn-save-wav');
  const btnConvertAnother = document.getElementById('btn-convert-another');

  const errorTitle = document.getElementById('error-title');
  const errorMsg   = document.getElementById('error-msg');
  const btnCopyLog = document.getElementById('btn-copy-log');
  const btnRetry   = document.getElementById('btn-retry');

  // ── State ──────────────────────────────────────────────────────────────────
  let currentFilePath  = null;
  let currentVideoInfo = null;
  let cropSelected     = false;
  let progressUnlisten = null;
  let logUnlisten      = null;
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
      barEncoding.style.width = pct + '%';
      pctEncoding.textContent = Math.round(pct) + '%';
    } else if (stage === 'packaging') {
      barPackaging.style.width = pct + '%';
      pctPackaging.textContent = Math.round(pct) + '%';
    }
    progressMsg.textContent = msg || '';
  }

  function resetProgress() {
    barEncoding.style.width  = '0%';
    barPackaging.style.width = '0%';
    pctEncoding.textContent  = '0%';
    pctPackaging.textContent = '0%';
    progressMsg.textContent  = '';
    consoleOutput.textContent = '';
  }

  function showError(title, msg) {
    errorTitle.textContent = title;
    errorMsg.textContent   = msg;
    showState('error');
  }

  const BADGE = {
    conformant:    { symbol: '✓', cls: 'badge-ok',      word: 'mantido'  },
    will_adjust:   { symbol: '↻', cls: 'badge-adjust',  word: 'ajustado' },
    needs_confirm: { symbol: '⚠', cls: 'badge-confirm', word: ''         },
  };

  function renderAttrs(info) {
    attrList.innerHTML = '';
    const attrs = [
      { key: 'codec',        label: 'Codec'        },
      { key: 'frame_rate',   label: 'Frame rate'   },
      { key: 'bitrate',      label: 'Bitrate'      },
      { key: 'pixel_format', label: 'Pixel format' },
      { key: 'resolution',   label: 'Resolução'    },
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

  // ── Core file handler ──────────────────────────────────────────────────────
  async function handleFile(filePath, originalName) {
    if (!filePath) return;
    currentFilePath  = filePath;
    lastOriginalName = (originalName || pathBasename(filePath)).replace(/\.[^.]+$/, '');
    cropSelected     = false;

    showState('analysis');
    attrList.innerHTML = '<div style="color:var(--text-secondary);font-size:13px">Analisando…</div>';
    cropConfirm.classList.add('hidden');
    analysisActions.classList.remove('hidden');
    btnStartConvert.disabled = false;

    try {
      currentVideoInfo = await tauri().core.invoke('analyze_video', { path: filePath });
      renderAttrs(currentVideoInfo);
      if (currentVideoInfo.needs_crop) {
        cropDescription.textContent = currentVideoInfo.crop_description || '';
        cropConfirm.classList.remove('hidden');
        btnStartConvert.disabled = true;
      }
    } catch (err) {
      showError(
        'Não foi possível ler o vídeo',
        String(err).includes('muito curto')
          ? String(err)
          : 'O arquivo pode estar corrompido, protegido ou em um formato não suportado.'
      );
    }
  }

  function pathBasename(p) {
    return (typeof p === 'string' ? p : String(p)).split(/[\\/]/).pop() || '';
  }

  // ── Native file picker ─────────────────────────────────────────────────────
  async function openFileDialog() {
    const path = await tauri().dialog.open({
      multiple: false,
      filters: [{ name: 'Vídeo', extensions: ['mp4', 'mov', 'webm', 'mkv', 'avi', 'm4v'] }],
    });
    if (path) handleFile(typeof path === 'string' ? path : path[0]);
  }

  btnPickFile.addEventListener('click', e => { e.stopPropagation(); openFileDialog(); });
  dropZone.addEventListener('click', openFileDialog);
  dropZone.addEventListener('keydown', e => { if (e.key === 'Enter' || e.key === ' ') openFileDialog(); });

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

  // ── Conversion ─────────────────────────────────────────────────────────────
  btnStartConvert.addEventListener('click', startConversion);
  btnPickAnother.addEventListener('click', () => showState('drop'));

  async function startConversion() {
    if (!currentFilePath) return;
    resetProgress();
    showState('processing');

    progressUnlisten = await tauri().event.listen('conversion-progress', ({ payload }) => {
      setProgress(payload.stage, payload.percent, payload.message);
    });
    logUnlisten = await tauri().event.listen('log-line', ({ payload }) => {
      appendLog(`[${payload.level.toUpperCase()}] ${payload.message}`);
    });

    try {
      await tauri().core.invoke('start_conversion', {
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
      if (String(err).includes('Cancelado')) {
        showState('drop');
      } else {
        showError('Erro durante a conversão', friendlyError(String(err)));
      }
    }
  }

  btnCancel.addEventListener('click', async () => {
    await tauri().core.invoke('cancel_conversion');
  });

  btnSaveWav.addEventListener('click', async () => {
    const suggestedName = lastOriginalName ? `${lastOriginalName}_ch15.wav` : 'libras_ch15.wav';
    const destPath = await tauri().dialog.save({
      defaultPath: suggestedName,
      filters: [{ name: 'Arquivo WAV', extensions: ['wav'] }],
    });
    if (!destPath) return;
    try {
      await tauri().core.invoke('save_wav', { destPath });
    } catch (err) {
      showError('Erro ao salvar o arquivo', friendlyError(String(err)));
    }
  });

  btnConvertAnother.addEventListener('click', () => {
    currentFilePath = null; currentVideoInfo = null;
    showState('drop');
  });

  btnCopyLog.addEventListener('click', async () => {
    const log = await tauri().core.invoke('get_log');
    await navigator.clipboard.writeText(log);
  });

  btnRetry.addEventListener('click', () => showState(currentFilePath ? 'analysis' : 'drop'));

  function friendlyError(raw) {
    if (raw.includes('Cancelado'))    return 'A operação foi cancelada.';
    if (raw.includes('muito curto'))  return raw;
    if (raw.includes('não coube') || raw.includes('grande demais'))
      return 'Um trecho do vídeo tem movimento ou detalhe demais e não coube no formato, mesmo no bitrate mínimo. Tente um vídeo com fundo mais simples ou em menor resolução.';
    if (raw.includes('permissão') || raw.includes('Permission'))
      return 'Sem permissão para gravar no destino. Tente salvar em outro local.';
    return 'Ocorreu um erro inesperado. Abra o log técnico para mais detalhes.';
  }

  // ── Public interface for main.js drag-drop routing ─────────────────────────
  return {
    setDragOver: (active) => dropZone.classList.toggle('drag-over', active),
    handleDrop:  (path)   => handleFile(path),
  };
}
