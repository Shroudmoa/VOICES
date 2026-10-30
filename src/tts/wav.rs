//! Reading the duration of a Piper WAV without pulling in an audio crate.

use std::fs::File;
use std::io::{BufReader, Read, Seek, SeekFrom};

#[derive(Debug, Clone)]
pub struct WavInfo {
    pub sample_rate: u32,
    pub channels: u16,
    pub bits_per_sample: u16,
    pub data_bytes: u64,
}

impl WavInfo {
    pub fn duration_secs(&self) -> f64 {
        let bytes_per_frame = self.channels as u64 * (self.bits_per_sample as u64 / 8);
        let frames = if bytes_per_frame == 0 {
            0.0
        } else {
            self.data_bytes as f64 / bytes_per_frame as f64
        };
        if self.sample_rate == 0 {
            0.0
        } else {
            frames / self.sample_rate as f64
        }
    }
}

pub fn read(path: &std::path::Path) -> anyhow::Result<WavInfo> {
    let file = File::open(path)?;
    let mut r = BufReader::new(file);
    let mut riff = [0u8; 12];
    r.read_exact(&mut riff)?;
    if &riff[0..4] != b"RIFF" || &riff[8..12] != b"WAVE" {
        anyhow::bail!("not a RIFF/WAVE file");
    }

    let mut info = WavInfo {
        sample_rate: 0,
        channels: 0,
        bits_per_sample: 0,
        data_bytes: 0,
    };

    loop {
        let mut hdr = [0u8; 8];
        match r.read_exact(&mut hdr) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
            Err(e) => return Err(e.into()),
        }
        let id = &hdr[0..4];
        let size = u32::from_le_bytes(hdr[4..8].try_into().unwrap()) as u64;

        if id == b"fmt " {
            let mut fmt = vec![0u8; size as usize];
            r.read_exact(&mut fmt)?;
            if size >= 16 {
                info.channels = u16::from_le_bytes(fmt[2..4].try_into().unwrap());
                info.sample_rate = u32::from_le_bytes(fmt[4..8].try_into().unwrap());
                info.bits_per_sample = u16::from_le_bytes(fmt[14..16].try_into().unwrap());
            }
        } else if id == b"data" {
            info.data_bytes = size;
            break;
        } else {
            r.seek(SeekFrom::Current(size as i64))?;
        }
    }

    if info.data_bytes == 0 || info.sample_rate == 0 {
        // Some encoders write 0xFFFFFFFF for streaming WAV. Fall back to the
        // file size minus a 44-byte header, assuming 22050 Hz mono 16-bit.
        let len = std::fs::metadata(path)?.len();
        info.data_bytes = len.saturating_sub(44);
        if info.sample_rate == 0 {
            info.sample_rate = 22050;
        }
        if info.channels == 0 {
            info.channels = 1;
        }
        if info.bits_per_sample == 0 {
            info.bits_per_sample = 16;
        }
    }
    Ok(info)
}

pub fn duration(path: &std::path::Path) -> anyhow::Result<f64> {
    Ok(read(path)?.duration_secs())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn write_wav(path: &std::path::Path, samples: usize) {
        let sr = 16000u32;
        let bytes = (samples * 2) as u32;
        let mut f = File::create(path).unwrap();
        f.write_all(b"RIFF").unwrap();
        f.write_all(&(36 + bytes).to_le_bytes()).unwrap();
        f.write_all(b"WAVE").unwrap();
        f.write_all(b"fmt ").unwrap();
        f.write_all(&16u32.to_le_bytes()).unwrap();
        f.write_all(&1u16.to_le_bytes()).unwrap();
        f.write_all(&1u16.to_le_bytes()).unwrap();
        f.write_all(&sr.to_le_bytes()).unwrap();
        f.write_all(&(sr * 2).to_le_bytes()).unwrap();
        f.write_all(&2u16.to_le_bytes()).unwrap();
        f.write_all(&16u16.to_le_bytes()).unwrap();
        f.write_all(b"data").unwrap();
        f.write_all(&bytes.to_le_bytes()).unwrap();
        f.write_all(&vec![0u8; samples * 2]).unwrap();
    }

    #[test]
    fn duration_is_read_back() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("t.wav");
        write_wav(&p, 16000);
        let d = duration(&p).unwrap();
        assert!((d - 1.0).abs() < 1e-3, "got {d}");
    }
}
