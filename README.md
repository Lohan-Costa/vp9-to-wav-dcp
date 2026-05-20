# VP9 para WAV — Acessibilidade para DCP

> Conversor de vídeo VP9 para WAV PCM conforme ISDCF Doc 13, para inclusão de trilha de Libras em DCPs de cinema.

<!-- Screenshot ou GIF do app aqui -->

---

## Para que serve

**DCPs** (Digital Cinema Packages) são o formato padrão de distribuição de filmes digitais para salas de cinema. Eles carregam áudio como trilhas PCM de 16 canais. O canal 15 dessa trilha pode ser usado para transportar dados — especificamente, um vídeo comprimido em VP9 com a tradução do filme em **Libras** (Língua Brasileira de Sinais).

O **ISDCF Document 13** define exatamente como fatiar esse vídeo VP9 em blocos de 2 segundos e empacotá-los como bytes PCM dentro de um arquivo WAV. Durante a exibição, um decodificador externo lê o canal 15 da saída AES/EBU da cabine de projeção, reconstrói o vídeo VP9 e o exibe em um monitor secundário da sala para o público surdo e com deficiência auditiva.

Este app automatiza todo esse processo de empacotamento — sem exigir que o masterizador de DCP conheça os detalhes técnicos da especificação.

---

## Download

Acesse a aba **[Releases](../../releases)** do repositório.

### macOS
1. Baixe o arquivo `.dmg` ou `.app.tar.gz`
2. Abra normalmente
3. Se o macOS bloquear com aviso de Gatekeeper:
   - Vá em **Preferências do Sistema → Segurança e Privacidade**
   - Clique em **"Abrir mesmo assim"** ao lado do nome do app

### Windows
1. Baixe o arquivo `.exe`
2. Execute diretamente (o app não precisa de instalação)
3. Se o Windows SmartScreen alertar:
   - Clique em **"Mais informações"**
   - Clique em **"Executar mesmo assim"**

---

## Como usar

### Aba Converter — gerar o WAV a partir de um vídeo

1. **Arraste o arquivo de vídeo** (mp4, mov, mkv, webm, avi) para a área de drop, ou clique em "Escolher arquivo"
2. O app analisa o vídeo e mostra um painel de verificação:
   - `✓` — atributo já está conforme, será mantido
   - `↻` — será ajustado automaticamente (codec, fps, bitrate, pixel format)
   - `⚠` — proporção diferente de 3:4 — o app oferece fazer um crop centralizado
3. Confirme o crop se necessário e clique em **Iniciar conversão**
4. Acompanhe o progresso nas duas barras (codificação VP9 + empacotamento PCM)
5. Ao finalizar, clique em **Salvar WAV** — o nome sugerido será `{nome_original}_ch15.wav`

### Aba Validar — verificar a integridade de um WAV

1. **Arraste o arquivo WAV** gerado por este app (ou por outra ferramenta compatível com ISDCF Doc 13)
2. O app verifica automaticamente:
   - Cabeçalho WAV (48 kHz, 24-bit, mono)
   - Magic numbers (`0xFFFFFFFF`) em todos os blocos
   - Estrutura de 288.000 bytes por bloco
   - EBML Headers válidos
   - VP9 Segments íntegros
3. O vídeo recuperado é exibido em um player embutido
4. Use **Copiar relatório técnico** para gerar um texto de QC para enviar ao cinema

---

## Especificações técnicas geradas

| Atributo | Valor |
|---|---|
| Codec de vídeo | VP9 (`libvpx-vp9`) |
| Resolução | 480 × 640 px (portrait) |
| Frame rate | 24.0 fps |
| Bitrate máximo | 1.0 Mbps |
| Pixel format | yuv420p / Y'UV |
| Container | WebM |
| Duração do chunk | 2 segundos |
| Formato WAV | Mono, 48 kHz, 24-bit PCM |
| Tamanho do bloco PCM | 288.000 bytes |
| Canal DCP | 15 |

---

## Solução de problemas

| Problema | O que fazer |
|---|---|
| "O vídeo é muito curto" | O vídeo precisa ter pelo menos 2 segundos |
| Crop inesperado | O vídeo não estava na proporção 3:4 — revise o enquadramento original |
| "Chunk grande demais" | O vídeo tem muita complexidade visual para 1 Mbps — reduza o tempo ou a resolução do vídeo de entrada |
| Checagem de EBML falhou | O WAV pode ter sido gerado com uma versão incompatível da ferramenta |
| App bloqueado no macOS/Windows | Siga as instruções da seção Download acima |

---

## Para desenvolvedores — build local

Pré-requisitos: [Node.js 20+](https://nodejs.org), [Rust stable](https://rustup.rs), [FFmpeg binários estáticos](https://ffmpeg.org/download.html)

```bash
# Clone o repositório
git clone https://github.com/lohancn/vp9-to-wav-dcp.git
cd vp9-to-wav-dcp

# Instale dependências JS
npm install

# Coloque os binários FFmpeg em src-tauri/binaries/ com os nomes corretos:
# macOS ARM:  ffmpeg-aarch64-apple-darwin  e  ffprobe-aarch64-apple-darwin
# macOS x64:  ffmpeg-x86_64-apple-darwin   e  ffprobe-x86_64-apple-darwin
# Windows:    ffmpeg-x86_64-pc-windows-msvc.exe  e  ffprobe-x86_64-pc-windows-msvc.exe

# Desenvolvimento (com hot-reload do frontend)
npm run dev

# Build de produção
npm run build
```

---

## Créditos

- **Idealização:** [Lohan Costa, edt.](https://www.linkedin.com/in/lohan-costa/)
- **Desenvolvimento:** Claude Opus 4.7
- **Especificação técnica:** [ISDCF Document 13 — Sign Language Video Encoding for Digital Cinema](http://isdcf.com/papers/ISDCF-Doc13-Sign-Language-Video-Encoding-for-Digital-Cinema.pdf)

---

## Licença

MIT © Lohan Costa
