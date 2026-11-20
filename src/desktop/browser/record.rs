//! Browser video recording.
//!
//! Frames come only from the browser's own paints (`on_paint`), never the
//! desktop. The page publishes its latest frame into a one-frame slot; the
//! encoder thread samples that slot at the chosen rate, so memory stays
//! bounded and a slow encoder drops frames instead of queueing them. Sampled
//! frames get monotonic timestamps from the recording clock; when the page
//! paints nothing new the previous frame repeats, so duration matches wall
//! time.
//!
//! The output is Motion JPEG in an AVI container (`.avi`), playable by VLC,
//! mpv, ffplay, Windows Media Player and browsers' `<video>` via ffmpeg-based
//! tools. It is written to `<name>.avi.partial` and renamed when finalized;
//! a failed recording removes its partial file.
use super::Frame;
use std::{
    io::{Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

#[derive(Clone, Copy, Debug)]
pub struct Limits {
    pub fps: u32,
    pub max_seconds: u32,
    pub max_width: u32,
    pub max_bytes: u64,
    pub cursor: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RecordingState {
    Recording,
    Finalizing,
    Completed,
    /// Stopped by a limit; the file is complete.
    StoppedAtLimit,
    Failed,
}

#[derive(Clone, Debug, serde::Serialize)]
pub struct Status {
    pub id: String,
    pub target: String,
    pub state: RecordingState,
    pub frames: u64,
    /// Sampling ticks skipped because encoding fell behind.
    pub dropped: u64,
    /// Ticks where the page had not painted a new frame (previous repeated).
    pub repeated: u64,
    pub duration_ms: u64,
    pub bytes: u64,
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    pub format: &'static str,
    pub path: Option<PathBuf>,
    pub stop_reason: Option<String>,
    pub error: Option<String>,
}

/// The page's side of a recording: where it publishes frames.
#[derive(Clone)]
pub struct Feed {
    pub latest: Arc<Mutex<Option<Arc<Frame>>>>,
    /// The virtual cursor in frame pixels, drawn when the recording asked for it.
    pub cursor: Arc<Mutex<Option<(f32, f32)>>>,
}

pub struct Recording {
    pub id: String,
    pub status: Arc<Mutex<Status>>,
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
    pub feed: Feed,
}

impl Recording {
    pub fn start(id: String, target: String, final_path: PathBuf, limits: Limits, wake: Arc<dyn Fn() + Send + Sync>) -> Self {
        let feed = Feed { latest: Default::default(), cursor: Default::default() };
        let status = Arc::new(Mutex::new(Status {
            id: id.clone(),
            target,
            state: RecordingState::Recording,
            frames: 0,
            dropped: 0,
            repeated: 0,
            duration_ms: 0,
            bytes: 0,
            width: 0,
            height: 0,
            fps: limits.fps,
            format: "avi (Motion JPEG)",
            path: None,
            stop_reason: None,
            error: None,
        }));
        let stop = Arc::new(AtomicBool::new(false));
        let thread = {
            let (status, stop, feed) = (status.clone(), stop.clone(), feed.clone());
            std::thread::Builder::new()
                .name("rodtty-recorder".into())
                .spawn(move || {
                    let result = encode(&final_path, limits, &feed, &stop, &status);
                    let mut s = status.lock().unwrap_or_else(|e| e.into_inner());
                    match result {
                        Ok(limit) => {
                            s.state = if limit.is_some() { RecordingState::StoppedAtLimit } else { RecordingState::Completed };
                            s.stop_reason = limit.or(s.stop_reason.take());
                            s.path = Some(final_path);
                        }
                        Err(error) => {
                            s.state = RecordingState::Failed;
                            s.error = Some(error);
                            s.path = None;
                        }
                    }
                    drop(s);
                    wake();
                })
                .ok()
        };
        Self { id, status, stop, thread, feed }
    }

    pub fn status(&self) -> Status {
        self.status.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    /// Ask the encoder to finish; `wait` blocks until the file is final.
    pub fn stop(&mut self, reason: &str, wait: bool) -> Status {
        {
            let mut s = self.status.lock().unwrap_or_else(|e| e.into_inner());
            if s.state == RecordingState::Recording {
                s.state = RecordingState::Finalizing;
                s.stop_reason = Some(reason.to_owned());
            }
        }
        self.stop.store(true, Ordering::Relaxed);
        if wait && let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
        self.status()
    }

    pub fn finished(&self) -> bool {
        !matches!(self.status().state, RecordingState::Recording | RecordingState::Finalizing)
    }
}

impl Drop for Recording {
    fn drop(&mut self) {
        self.stop("target closed", true);
    }
}

/// Scale the BGRA frame down to at most `max_width` (keeping even sizes, as
/// players prefer), convert to RGB and draw the cursor.
fn prepare(frame: &Frame, max_width: u32, cursor: Option<(f32, f32)>) -> (u32, u32, Vec<u8>) {
    let factor = (frame.width as f32 / max_width as f32).max(1.0);
    let width = (((frame.width as f32 / factor) as u32) & !1).max(2);
    let height = (((frame.height as f32 / factor) as u32) & !1).max(2);
    let mut rgb = vec![0u8; (width * height * 3) as usize];
    for y in 0..height {
        let sy = ((y as f32 * factor) as u32).min(frame.height - 1);
        for x in 0..width {
            let sx = ((x as f32 * factor) as u32).min(frame.width - 1);
            let s = ((sy * frame.width + sx) * 4) as usize;
            let d = ((y * width + x) * 3) as usize;
            rgb[d] = frame.bgra[s + 2];
            rgb[d + 1] = frame.bgra[s + 1];
            rgb[d + 2] = frame.bgra[s];
        }
    }
    if let Some((cx, cy)) = cursor {
        super::draw_cursor_rgb(&mut rgb, width, height, cx / factor, cy / factor, 3);
    }
    (width, height, rgb)
}

fn jpeg(width: u32, height: u32, rgb: &[u8]) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 80)
        .encode(rgb, width, height, image::ExtendedColorType::Rgb8)
        .map_err(|e| e.to_string())?;
    Ok(out)
}

/// Run the encoder until stopped or a limit is hit. Returns the limit that
/// stopped it, if any.
fn encode(final_path: &Path, limits: Limits, feed: &Feed, stop: &AtomicBool, status: &Mutex<Status>) -> Result<Option<String>, String> {
    let partial = final_path.with_extension("avi.partial");
    let result = (|| {
        let interval = Duration::from_secs_f64(1.0 / f64::from(limits.fps.max(1)));
        let started = Instant::now();
        // Wait for the first frame to fix the video size.
        let first = loop {
            if let Some(frame) = feed.latest.lock().unwrap_or_else(|e| e.into_inner()).clone() {
                break frame;
            }
            if stop.load(Ordering::Relaxed) {
                return Err("The browser painted no frame before the recording stopped".to_owned());
            }
            if started.elapsed() > Duration::from_secs(10) {
                return Err("The browser painted no frame within 10 seconds".to_owned());
            }
            std::thread::sleep(Duration::from_millis(20));
        };
        let cursor = || if limits.cursor { *feed.cursor.lock().unwrap_or_else(|e| e.into_inner()) } else { None };
        let (width, height, _) = prepare(&first, limits.max_width, None);
        let mut writer = AviWriter::create(&partial, width, height, limits.fps).map_err(|e| e.to_string())?;
        {
            let mut s = status.lock().unwrap_or_else(|e| e.into_inner());
            s.width = width;
            s.height = height;
        }
        let clock = Instant::now();
        let mut tick: u64 = 0;
        let mut last_id = u64::MAX;
        let mut last_jpeg: Option<Vec<u8>> = None;
        let mut limit = None;
        loop {
            if stop.load(Ordering::Relaxed) {
                break;
            }
            let due = clock + interval.mul_f64(tick as f64);
            let now = Instant::now();
            if due > now {
                std::thread::sleep(due - now);
            }
            // Ticks that passed while encoding are dropped, not queued.
            let elapsed_ticks = (clock.elapsed().as_secs_f64() / interval.as_secs_f64()) as u64;
            if elapsed_ticks > tick + 1 {
                let skipped = elapsed_ticks - tick - 1;
                status.lock().unwrap_or_else(|e| e.into_inner()).dropped += skipped;
                // Keep the timeline: repeat the last frame for skipped ticks.
                if let Some(jpeg) = &last_jpeg {
                    for _ in 0..skipped {
                        writer.frame(jpeg).map_err(|e| e.to_string())?;
                    }
                }
                tick = elapsed_ticks - 1;
            }
            let frame = feed.latest.lock().unwrap_or_else(|e| e.into_inner()).clone();
            let data = match frame {
                Some(frame) if frame.id != last_id || limits.cursor => {
                    let repeated = frame.id == last_id;
                    last_id = frame.id;
                    // A resize changes the page's frame size; the video keeps
                    // its first size and scales into it.
                    let (w, h, rgb) = prepare(&frame, limits.max_width, cursor());
                    let scaled = if (w, h) == (width, height) { rgb } else { resize_rgb(&rgb, w, h, width, height) };
                    let encoded = jpeg(width, height, &scaled)?;
                    if repeated {
                        status.lock().unwrap_or_else(|e| e.into_inner()).repeated += 1;
                    }
                    last_jpeg = Some(encoded.clone());
                    encoded
                }
                _ => {
                    status.lock().unwrap_or_else(|e| e.into_inner()).repeated += 1;
                    last_jpeg.clone().ok_or("no frame")?
                }
            };
            writer.frame(&data).map_err(|e| format!("Writing the video failed: {e}"))?;
            tick += 1;
            let mut s = status.lock().unwrap_or_else(|e| e.into_inner());
            s.frames = writer.frames;
            s.bytes = writer.bytes;
            s.duration_ms = writer.frames * 1000 / u64::from(limits.fps.max(1));
            drop(s);
            if writer.frames >= u64::from(limits.max_seconds) * u64::from(limits.fps) {
                limit = Some(format!("Reached the {} second limit", limits.max_seconds));
                break;
            }
            if writer.bytes >= limits.max_bytes {
                limit = Some("Reached the size limit".to_owned());
                break;
            }
        }
        if writer.frames == 0 {
            writer.frame(&jpeg(width, height, &prepare(&first, limits.max_width, cursor()).2)?).map_err(|e| e.to_string())?;
        }
        let written = writer.bytes;
        writer.finish().map_err(|e| format!("Finalizing the video failed: {e}"))?;
        std::fs::rename(&partial, final_path).map_err(|e| e.to_string())?;
        let mut s = status.lock().unwrap_or_else(|e| e.into_inner());
        s.bytes = std::fs::metadata(final_path).map(|m| m.len()).unwrap_or(written);
        Ok(limit)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&partial);
        let _ = std::fs::remove_file(final_path);
    }
    result
}

fn resize_rgb(rgb: &[u8], w: u32, h: u32, to_w: u32, to_h: u32) -> Vec<u8> {
    let mut out = vec![0u8; (to_w * to_h * 3) as usize];
    for y in 0..to_h {
        let sy = (y * h / to_h).min(h - 1);
        for x in 0..to_w {
            let sx = (x * w / to_w).min(w - 1);
            let s = ((sy * w + sx) * 3) as usize;
            let d = ((y * to_w + x) * 3) as usize;
            out[d..d + 3].copy_from_slice(&rgb[s..s + 3]);
        }
    }
    out
}

/// A minimal AVI 1.0 writer for one Motion JPEG video stream.
pub struct AviWriter {
    file: std::io::BufWriter<std::fs::File>,
    index: Vec<(u32, u32)>,
    movi_start: u64,
    /// The next chunk's offset from the `movi` fourcc.
    next_offset: u32,
    pub frames: u64,
    pub bytes: u64,
    max_frame: u32,
    width: u32,
    height: u32,
    fps: u32,
}

fn le32(v: u32) -> [u8; 4] {
    v.to_le_bytes()
}

impl AviWriter {
    const HEADER: u64 = 12 + 8 + 4 + (8 + 56) + 8 + 4 + (8 + 56) + (8 + 40) + 12;

    pub fn create(path: &Path, width: u32, height: u32, fps: u32) -> std::io::Result<Self> {
        let file = std::fs::File::create(path)?;
        let mut writer = Self {
            file: std::io::BufWriter::new(file),
            index: Vec::new(),
            movi_start: 0,
            next_offset: 4,
            frames: 0,
            bytes: 0,
            max_frame: 0,
            width,
            height,
            fps,
        };
        writer.headers()?;
        writer.movi_start = Self::HEADER - 4;
        Ok(writer)
    }

    fn headers(&mut self) -> std::io::Result<()> {
        let f = &mut self.file;
        let frames = self.frames as u32;
        let movi_size = self.next_offset;
        let riff_size = (Self::HEADER - 8) as u32 + movi_size - 4 + 8 + 16 * self.index.len() as u32;
        f.write_all(b"RIFF")?;
        f.write_all(&le32(riff_size))?;
        f.write_all(b"AVI ")?;
        f.write_all(b"LIST")?;
        f.write_all(&le32(4 + (8 + 56) + 8 + 4 + (8 + 56) + (8 + 40)))?;
        f.write_all(b"hdrl")?;
        f.write_all(b"avih")?;
        f.write_all(&le32(56))?;
        for v in [1_000_000 / self.fps.max(1), self.max_frame * self.fps, 0, 0x10, frames, 0, 1, self.max_frame, self.width, self.height, 0, 0, 0, 0] {
            f.write_all(&le32(v))?;
        }
        f.write_all(b"LIST")?;
        f.write_all(&le32(4 + (8 + 56) + (8 + 40)))?;
        f.write_all(b"strl")?;
        f.write_all(b"strh")?;
        f.write_all(&le32(56))?;
        f.write_all(b"vidsMJPG")?;
        for v in [0u32, 0, 0, 1, self.fps, 0, frames, self.max_frame, u32::MAX, 0] {
            f.write_all(&le32(v))?;
        }
        for v in [0u16, 0, self.width as u16, self.height as u16] {
            f.write_all(&v.to_le_bytes())?;
        }
        f.write_all(b"strf")?;
        f.write_all(&le32(40))?;
        f.write_all(&le32(40))?;
        f.write_all(&(self.width as i32).to_le_bytes())?;
        f.write_all(&(self.height as i32).to_le_bytes())?;
        f.write_all(&1u16.to_le_bytes())?;
        f.write_all(&24u16.to_le_bytes())?;
        f.write_all(b"MJPG")?;
        for v in [self.width * self.height * 3, 0, 0, 0, 0] {
            f.write_all(&le32(v))?;
        }
        f.write_all(b"LIST")?;
        f.write_all(&le32(movi_size))?;
        f.write_all(b"movi")?;
        Ok(())
    }

    pub fn frame(&mut self, jpeg: &[u8]) -> std::io::Result<()> {
        let size = jpeg.len() as u32;
        let offset = self.next_offset;
        self.next_offset += 8 + size + (size & 1);
        self.file.write_all(b"00dc")?;
        self.file.write_all(&le32(size))?;
        self.file.write_all(jpeg)?;
        if size & 1 == 1 {
            self.file.write_all(&[0])?;
        }
        self.index.push((offset, size));
        self.frames += 1;
        self.max_frame = self.max_frame.max(size);
        self.bytes += u64::from(size) + 8;
        Ok(())
    }

    pub fn finish(mut self) -> std::io::Result<()> {
        self.file.write_all(b"idx1")?;
        self.file.write_all(&le32(16 * self.index.len() as u32))?;
        for (offset, size) in &self.index {
            self.file.write_all(b"00dc")?;
            self.file.write_all(&le32(0x10))?;
            self.file.write_all(&le32(*offset))?;
            self.file.write_all(&le32(*size))?;
        }
        self.file.seek(SeekFrom::Start(0))?;
        self.headers()?;
        let _ = self.movi_start;
        self.file.flush()?;
        self.file.get_ref().sync_all()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_avi_file_has_consistent_sizes_and_an_index() {
        let dir = crate::test_support::tempdir().unwrap();
        let path = dir.path().join("v.avi");
        let mut w = AviWriter::create(&path, 4, 2, 10).unwrap();
        let rgb = vec![200u8; 4 * 2 * 3];
        for _ in 0..3 {
            w.frame(&jpeg(4, 2, &rgb).unwrap()).unwrap();
        }
        w.finish().unwrap();
        let bytes = std::fs::read(&path).unwrap();
        assert_eq!(&bytes[..4], b"RIFF");
        assert_eq!(u32::from_le_bytes(bytes[4..8].try_into().unwrap()) as usize, bytes.len() - 8);
        assert_eq!(&bytes[8..12], b"AVI ");
        let frames = u32::from_le_bytes(bytes[48..52].try_into().unwrap());
        assert_eq!(frames, 3);
        assert!(bytes.windows(4).any(|w| w == b"idx1"));
    }
}
