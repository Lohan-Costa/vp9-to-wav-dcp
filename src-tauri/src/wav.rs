use anyhow::{anyhow, Result};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::Path;

// WAV header layout (44 bytes, all fields little-endian unless noted):
//   0..4   "RIFF"
//   4..8   file_size - 8
//   8..12  "WAVE"
//  12..16  "fmt "
//  16..20  16  (fmt chunk size)
//  20..22  1   (PCM)
//  22..24  1   (mono)
//  24..28  48000
//  28..32  144000  (byte rate = 48000 * 1 * 3)
//  32..34  3   (block align = 1 * 3)
//  34..36  24  (bits per sample)
//  36..40  "data"
//  40..44  data_size
//  44+     PCM data

const HEADER_SIZE: u64 = 44;
const SAMPLE_RATE: u32 = 48_000;
const BITS_PER_SAMPLE: u16 = 24;
const CHANNELS: u16 = 1;
const BYTE_RATE: u32 = SAMPLE_RATE * CHANNELS as u32 * (BITS_PER_SAMPLE as u32 / 8);
const BLOCK_ALIGN: u16 = CHANNELS * (BITS_PER_SAMPLE / 8);

// ── Writer ────────────────────────────────────────────────────────────────────

/// Write a WAV file by streaming PCM blocks from an iterator.
/// Returns the number of bytes written to the data chunk.
pub fn write_wav<I>(path: &Path, blocks: I) -> Result<u64>
where
    I: Iterator<Item = Result<Vec<u8>>>,
{
    let mut file = std::fs::File::create(path)?;

    // Write placeholder header (sizes filled in later)
    file.write_all(&placeholder_header())?;

    let mut data_size: u64 = 0;
    for block in blocks {
        let b = block?;
        file.write_all(&b)?;
        data_size += b.len() as u64;
    }

    // Seek back and fill in real sizes
    let file_size = (HEADER_SIZE - 8) + data_size;
    file.seek(SeekFrom::Start(4))?;
    file.write_all(&(file_size as u32).to_le_bytes())?;
    file.seek(SeekFrom::Start(40))?;
    file.write_all(&(data_size as u32).to_le_bytes())?;

    Ok(data_size)
}

// ── Reader ────────────────────────────────────────────────────────────────────

pub struct WavData {
    pub sample_rate: u32,
    pub bits_per_sample: u16,
    pub channels: u16,
    pub data_offset: u64,
    pub data_size: u64,
}

/// Open a WAV file, validate its header and return metadata.
/// Does NOT load the full data into memory.
pub fn read_wav_header(path: &Path) -> Result<WavData> {
    let mut f = std::fs::File::open(path)?;
    let mut header = [0u8; 44];
    f.read_exact(&mut header)?;

    if &header[0..4] != b"RIFF" {
        return Err(anyhow!("Este arquivo não é um WAV válido (marcador RIFF ausente)."));
    }
    if &header[8..12] != b"WAVE" {
        return Err(anyhow!("Este arquivo não é um WAV válido (marcador WAVE ausente)."));
    }
    if &header[12..16] != b"fmt " {
        return Err(anyhow!("Este WAV não tem o sub-chunk 'fmt ' no local esperado."));
    }
    if &header[36..40] != b"data" {
        return Err(anyhow!("Este WAV não tem o sub-chunk 'data' no local esperado."));
    }

    let audio_format = u16::from_le_bytes(header[20..22].try_into().unwrap());
    if audio_format != 1 {
        return Err(anyhow!(
            "Este WAV está comprimido (formato {}). É necessário WAV PCM linear (formato 1).",
            audio_format
        ));
    }

    let channels       = u16::from_le_bytes(header[22..24].try_into().unwrap());
    let sample_rate    = u32::from_le_bytes(header[24..28].try_into().unwrap());
    let bits_per_sample= u16::from_le_bytes(header[34..36].try_into().unwrap());
    let data_size      = u32::from_le_bytes(header[40..44].try_into().unwrap()) as u64;

    Ok(WavData {
        sample_rate,
        bits_per_sample,
        channels,
        data_offset: HEADER_SIZE,
        data_size,
    })
}

/// Read raw PCM data bytes from a WAV file.
pub fn read_wav_data(path: &Path) -> Result<(WavData, Vec<u8>)> {
    let meta = read_wav_header(path)?;
    let mut f = std::fs::File::open(path)?;
    f.seek(SeekFrom::Start(meta.data_offset))?;
    let mut data = vec![0u8; meta.data_size as usize];
    f.read_exact(&mut data)?;
    Ok((meta, data))
}

// ── Internal ──────────────────────────────────────────────────────────────────

fn placeholder_header() -> Vec<u8> {
    let mut h = Vec::with_capacity(44);
    h.extend_from_slice(b"RIFF");
    h.extend_from_slice(&0u32.to_le_bytes()); // placeholder file size
    h.extend_from_slice(b"WAVE");
    h.extend_from_slice(b"fmt ");
    h.extend_from_slice(&16u32.to_le_bytes());
    h.extend_from_slice(&1u16.to_le_bytes()); // PCM
    h.extend_from_slice(&CHANNELS.to_le_bytes());
    h.extend_from_slice(&SAMPLE_RATE.to_le_bytes());
    h.extend_from_slice(&BYTE_RATE.to_le_bytes());
    h.extend_from_slice(&BLOCK_ALIGN.to_le_bytes());
    h.extend_from_slice(&BITS_PER_SAMPLE.to_le_bytes());
    h.extend_from_slice(b"data");
    h.extend_from_slice(&0u32.to_le_bytes()); // placeholder data size
    h
}

// ── Validation helpers ────────────────────────────────────────────────────────

pub fn validate_wav_format(meta: &WavData) -> Option<String> {
    if meta.sample_rate == 48_000 && meta.bits_per_sample == 24 && meta.channels == 1 {
        None
    } else {
        Some(format!(
            "Este WAV está no formato {} Hz, {}-bit, {} canal(is). \
             Para conter vídeo ISDCF Doc 13, ele precisa ser 48.000 Hz, 24-bit, mono.",
            meta.sample_rate, meta.bits_per_sample, meta.channels
        ))
    }
}
