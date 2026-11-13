//! Author avatars from `https://www.gravatar.com/avatar/<md5(lowercase email)>?d=404&s=N`.
//!
//! Images download on a background thread into a disk cache next to the settings; until one is ready (or when
//! the download fails) callers draw the initials fallback. A failed download (offline, a server error, a
//! timeout) is tried again after a back-off; an address without a Gravatar is remembered on disk for a week.
//!
//! The worker touches only [`Shared`] and the cache folder (never a texture), so it can be left running when the
//! application exits: stopping it does not wait for a download under way.

use std::collections::{HashMap, VecDeque};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::thread;
use std::time::{Duration, Instant};

use dear_imgui_rs::TextureId;
use imgit_git::Author;

use crate::gfx::Textures;
use crate::gfx::image::{Image, load_image};
use crate::theme::Color;
use crate::ui::Cx;
use crate::ui::draw::DrawList;
use crate::ui::geom::{Vec2, vec2};

#[derive(Debug)]
struct Request {
    key: String,
    email: String,
    size: u32,
    /// How many earlier tries failed.
    attempts: u32,
}

#[derive(Debug)]
enum Entry {
    /// Waiting for the worker (after `attempts` failed tries).
    Queued {
        attempts: u32,
    },
    /// Decoded by the worker, waiting for its texture upload on the next frame that asks for it.
    Decoded(Image),
    Ready(TextureId),
    /// No Gravatar for the address, or no cache folder: the initials stay.
    Missing,
    /// No network, a server error, a timeout or not an image: the initials stay until `retry_due`.
    Failed {
        since: Instant,
        attempts: u32,
    },
}

/// How long after its `attempts`-th failure a download is tried again: a minute, doubling up to a quarter hour.
fn retry_delay(attempts: u32) -> Duration {
    const FIRST: u64 = 60;
    const CAP: u64 = 15 * 60;
    let doublings = attempts.saturating_sub(1).min(16);
    Duration::from_secs((FIRST << doublings).min(CAP))
}

/// Whether a download that failed `attempts` times, `elapsed` ago, is tried again now.
fn retry_due(elapsed: Duration, attempts: u32) -> bool {
    elapsed >= retry_delay(attempts)
}

/// How long the "no Gravatar" marker file is believed: a newly created Gravatar shows up after that.
const NONE_MARKER_LIFETIME: Duration = Duration::from_secs(7 * 24 * 60 * 60);

/// Whether a `.none` marker of this age no longer holds.
fn none_marker_expired(age: Duration) -> bool {
    age >= NONE_MARKER_LIFETIME
}

#[derive(Debug, Default)]
struct State {
    queue: VecDeque<Request>,
    entries: HashMap<String, Entry>,
    cache_dir: Option<PathBuf>,
    /// Counts the workers stopped so far: a worker started under an older number ends and leaves no trace.
    generation: u64,
}

#[derive(Debug, Default)]
struct Shared {
    state: Mutex<State>,
    wake: Condvar,
    /// Requests queued or being processed.
    in_flight: AtomicUsize,
}

impl Shared {
    /// The state; a worker that panicked leaves it usable (every update is a single assignment).
    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// The avatar downloads and their textures, in [`UiState`](crate::ui::UiState).
#[derive(Debug)]
pub struct Avatars {
    shared: Arc<Shared>,
    /// Whether a worker of the current generation runs.
    worker: bool,
    enabled: bool,
}

impl Default for Avatars {
    fn default() -> Self {
        Self::new()
    }
}

/// The address Gravatar hashes: without the spaces an author's email sometimes carries around it, lower case.
fn gravatar_hash(email: &str) -> String {
    let trimmed = email.trim_matches([' ', '\t', '\r', '\n']);
    imgit_app::md5_hex(trimmed.to_ascii_lowercase())
}

/// `d=404`: an address without a Gravatar gets the initials instead of a generated picture.
fn gravatar_url(hash: &str, size: u32) -> String {
    format!("https://www.gravatar.com/avatar/{hash}?d=404&s={size}")
}

/// `file` with `suffix` appended to its name.
fn with_suffix(file: &Path, suffix: &str) -> PathBuf {
    let mut name = file.as_os_str().to_owned();
    name.push(suffix);
    PathBuf::from(name)
}

/// The HTTP client of the worker thread: its connection to gravatar.com is kept alive between avatars.
fn http_agent() -> ureq::Agent {
    use ureq::tls::{RootCerts, TlsConfig, TlsProvider};
    // The system proxy, as the Windows download API used before honoured it.
    let proxy = imgit_platform::system::system_proxy()
        .and_then(|proxy| ureq::Proxy::new(&proxy).ok())
        .or_else(ureq::Proxy::try_from_env);
    ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(30)))
        .http_status_as_error(false)
        .proxy(proxy)
        .tls_config(
            TlsConfig::builder()
                .provider(TlsProvider::NativeTls)
                .root_certs(RootCerts::PlatformVerifier)
                .build(),
        )
        .build()
        .new_agent()
}

/// What a download came to.
enum Download {
    Done,
    /// The server has no Gravatar for the address.
    Missing,
    Failed,
}

/// GETs `url` into `file` (through a temporary name, so a cut-off download is never taken for an image).
fn download(agent: &ureq::Agent, url: &str, file: &Path) -> Download {
    let Ok(mut response) = agent.get(url).call() else {
        return Download::Failed;
    };
    match response.status().as_u16() {
        // No Gravatar for the address: remember it, so the initials are drawn without asking again.
        404 => {
            let _ = fs::File::create(with_suffix(file, ".none"));
            return Download::Missing;
        }
        200 => {}
        _ => return Download::Failed,
    }
    let Ok(body) = response.body_mut().read_to_vec() else {
        return Download::Failed;
    };
    if body.is_empty() {
        return Download::Failed;
    }
    let partial = with_suffix(file, ".part");
    if fs::write(&partial, body).is_ok() && fs::rename(&partial, file).is_ok() {
        Download::Done
    } else {
        Download::Failed
    }
}

/// Whether the `.none` marker of an address is there and still believed; an expired one is removed.
fn has_none_marker(marker: &Path) -> bool {
    let Ok(modified) = fs::metadata(marker).and_then(|meta| meta.modified()) else {
        return false;
    };
    let age = modified.elapsed().unwrap_or(Duration::ZERO);
    if none_marker_expired(age) {
        let _ = fs::remove_file(marker);
        return false;
    }
    true
}

fn process(agent: &ureq::Agent, cache_dir: Option<&Path>, request: &Request) -> Entry {
    let failed = || Entry::Failed {
        since: Instant::now(),
        attempts: request.attempts + 1,
    };
    let Some(cache_dir) = cache_dir else {
        return Entry::Missing;
    };
    let hash = gravatar_hash(&request.email);
    let _ = fs::create_dir_all(cache_dir);
    let file = cache_dir.join(format!("{hash}-{}.png", request.size));
    if !file.exists() {
        if has_none_marker(&with_suffix(&file, ".none")) {
            return Entry::Missing;
        }
        match download(agent, &gravatar_url(&hash, request.size), &file) {
            Download::Done => {}
            Download::Missing => return Entry::Missing,
            Download::Failed => return failed(),
        }
    }
    match load_image(&file, None) {
        Some(image) => Entry::Decoded(image),
        None => {
            // A file that does not decode (a bad download) goes: the retry downloads it again.
            let _ = fs::remove_file(&file);
            failed()
        }
    }
}

/// The worker of `generation`: ends when `stop_worker` has moved the count on, whatever it is doing by then.
fn worker(shared: &Shared, generation: u64) {
    let agent = http_agent();
    loop {
        let (request, cache_dir) = {
            let mut state = shared.lock();
            loop {
                if state.generation != generation {
                    return;
                }
                if let Some(request) = state.queue.pop_front() {
                    break (request, state.cache_dir.clone());
                }
                state = shared
                    .wake
                    .wait(state)
                    .unwrap_or_else(PoisonError::into_inner);
            }
        };
        let entry = process(&agent, cache_dir.as_deref(), &request);
        let mut state = shared.lock();
        if state.generation != generation {
            // Stopped meanwhile: `shutdown` has reset the entries and the count.
            return;
        }
        state.entries.insert(request.key, entry);
        shared.in_flight.fetch_sub(1, Ordering::SeqCst);
    }
}

impl Avatars {
    /// No cache directory yet (every avatar stays its initials until one is set), downloads enabled.
    #[must_use]
    pub fn new() -> Self {
        Self {
            shared: Arc::default(),
            worker: false,
            enabled: true,
        }
    }

    /// Where downloaded avatars are kept (the profile's "gravatars" folder); `None` turns downloads off.
    pub fn set_cache_directory(&mut self, directory: Option<PathBuf>) {
        self.shared.lock().cache_dir = directory;
    }

    /// Whether Gravatars are shown at all (the preference); off, every avatar is its initials.
    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
    }

    /// The texture for `email` at `size` device pixels, or `None` while it is not available. The first call
    /// for an address queues its download.
    pub fn texture(
        &mut self,
        textures: &mut dyn Textures,
        email: &str,
        size: u32,
    ) -> Option<TextureId> {
        // An empty email asks Gravatar for md5("").
        if !self.enabled || size == 0 {
            return None;
        }
        let key = format!("{}#{size}", email.to_ascii_lowercase());
        let mut state = self.shared.lock();
        // A first request, or a failed download whose back-off is over, goes to the worker.
        let attempts = match state.entries.get_mut(&key) {
            None => 0,
            Some(Entry::Failed { since, attempts }) if retry_due(since.elapsed(), *attempts) => {
                *attempts
            }
            Some(entry) => {
                if let Entry::Decoded(image) = entry {
                    *entry = match textures.create_texture(image.width, image.height, &image.rgba) {
                        Ok(texture) => Entry::Ready(texture),
                        Err(_) => Entry::Failed {
                            since: Instant::now(),
                            attempts: 1,
                        },
                    };
                }
                return match entry {
                    Entry::Ready(texture) => Some(*texture),
                    _ => None,
                };
            }
        };
        state
            .entries
            .insert(key.clone(), Entry::Queued { attempts });
        state.queue.push_back(Request {
            key,
            email: email.to_owned(),
            size,
            attempts,
        });
        self.shared.in_flight.fetch_add(1, Ordering::SeqCst);
        let generation = state.generation;
        drop(state);
        if !self.worker {
            let shared = Arc::clone(&self.shared);
            self.worker = thread::Builder::new()
                .name("avatars".to_owned())
                .spawn(move || worker(&shared, generation))
                .is_ok();
        }
        self.shared.wake.notify_one();
        None
    }

    /// Whether downloads are queued or running, or an image waits for its upload (the headless screenshot
    /// mode waits for them).
    #[must_use]
    pub fn pending(&self) -> bool {
        // Without a worker nothing moves: the queue only drains while one runs.
        if !self.worker {
            return false;
        }
        self.shared.in_flight.load(Ordering::SeqCst) > 0
            || self
                .shared
                .lock()
                .entries
                .values()
                .any(|entry| matches!(entry, Entry::Decoded(_)))
    }

    /// Tells the worker to end and lets go of it: a download under way (up to the client's 30 s timeout) is
    /// not waited for, and its result is dropped. Safe, as the worker uses no texture or window.
    fn stop_worker(&mut self) {
        {
            let mut state = self.shared.lock();
            state.generation += 1;
            state.queue.clear();
        }
        self.shared.wake.notify_all();
        self.worker = false;
    }

    /// Stops the worker (without waiting for a download under way) and frees the textures. The cache can be
    /// used again afterwards.
    pub fn shutdown(&mut self, textures: &mut dyn Textures) {
        self.stop_worker();
        let mut state = self.shared.lock();
        for (_, entry) in state.entries.drain() {
            if let Entry::Ready(texture) = entry {
                textures.destroy_texture(texture);
            }
        }
        state.queue.clear();
        self.shared.in_flight.store(0, Ordering::SeqCst);
    }
}

impl Drop for Avatars {
    fn drop(&mut self) {
        self.stop_worker();
    }
}

/// The initials an avatar shows: the first letters of the first two words of the author's name (of the email's
/// local part without one), in upper case; "?" when there are none.
#[must_use]
pub fn initials(author: &Author) -> String {
    let source = if author.name.is_empty() {
        &author.email
    } else {
        &author.name
    };
    let mut result = String::new();
    let mut word_start = true;
    for byte in source.bytes() {
        if matches!(byte, b' ' | b'.' | b'-' | b'_' | b'@') {
            word_start = true;
            if byte == b'@' {
                break;
            }
            continue;
        }
        if word_start && byte.is_ascii_alphanumeric() {
            result.push(char::from(byte.to_ascii_uppercase()));
            if result.len() == 2 {
                break;
            }
        }
        word_start = false;
    }
    if result.is_empty() {
        result.push('?');
    }
    result
}

/// `ImGui::ColorConvertHSVtoRGB`.
fn hsv_to_rgb(h: f32, s: f32, v: f32) -> Color {
    if s == 0.0 {
        return Color::new(v, v, v, 1.0);
    }
    let h = h.rem_euclid(1.0) / (60.0 / 360.0);
    let sector = h as i32;
    let f = h - sector as f32;
    let p = v * (1.0 - s);
    let q = v * (1.0 - s * f);
    let t = v * (1.0 - s * (1.0 - f));
    let (r, g, b) = match sector {
        0 => (v, t, p),
        1 => (q, v, p),
        2 => (p, v, t),
        3 => (p, q, v),
        4 => (t, p, v),
        _ => (v, p, q),
    };
    Color::new(r, g, b, 1.0)
}

/// The disc colour of an address: a hue from its FNV-1a hash (case ignored); `None` for an empty address,
/// which gets the theme's default avatar colour.
#[must_use]
pub fn email_color(email: &str) -> Option<Color> {
    if email.is_empty() {
        return None;
    }
    let hash = email.bytes().fold(2_166_136_261_u32, |hash, byte| {
        (hash ^ u32::from(byte.to_ascii_lowercase())).wrapping_mul(16_777_619)
    });
    Some(hsv_to_rgb((hash % 360) as f32 / 360.0, 0.45, 0.62))
}

impl Cx<'_> {
    /// The Gravatar texture for `email` at `size` device pixels, once it is there.
    pub fn avatar_texture(&mut self, email: &str, size: u32) -> Option<TextureId> {
        self.state
            .avatars
            .texture(&mut *self.shell.textures, email, size)
    }

    /// The disc colour of an author without a Gravatar.
    #[must_use]
    pub fn avatar_color(&self, email: &str) -> Color {
        email_color(email).unwrap_or(self.palette.avatar_default)
    }

    /// The avatar: the author's Gravatar, or a coloured disc with their initials (without one, or until it
    /// loads). `rounding` `None` draws a circle.
    pub fn draw_avatar(
        &mut self,
        draw: DrawList<'_>,
        center: Vec2,
        diameter: f32,
        author: &Author,
        rounding: Option<f32>,
    ) {
        let radius = diameter / 2.0;
        let corner = rounding.unwrap_or(radius);
        let min = center - Vec2::splat(radius);
        let max = center + Vec2::splat(radius);
        if let Some(texture) = self.avatar_texture(&author.email, diameter.round().max(0.0) as u32)
        {
            draw.add_image_rounded(
                texture,
                min,
                max,
                Vec2::ZERO,
                vec2(1.0, 1.0),
                Color::WHITE,
                corner,
            );
            return;
        }
        draw.add_rect_filled(min, max, self.avatar_color(&author.email), corner);
        let text = initials(author);
        let font = self.fonts.semibold;
        let font_size = diameter * 0.42 / self.scale;
        let size = self.text_size(font, font_size, &text);
        draw.add_text(
            font,
            self.font_px(font, font_size),
            center - size / 2.0,
            self.palette.white,
            &text,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gfx::Error;

    #[derive(Default)]
    struct FakeTextures {
        created: Vec<(u32, u32)>,
        destroyed: Vec<TextureId>,
    }

    impl Textures for FakeTextures {
        fn create_texture(
            &mut self,
            width: u32,
            height: u32,
            _: &[u8],
        ) -> Result<TextureId, Error> {
            self.created.push((width, height));
            Ok(TextureId::new(self.created.len() as u64))
        }

        fn destroy_texture(&mut self, texture: TextureId) {
            self.destroyed.push(texture);
        }
    }

    fn author(name: &str, email: &str) -> Author {
        Author {
            name: name.to_owned(),
            email: email.to_owned(),
        }
    }

    fn wait_for(avatars: &Avatars) {
        for _ in 0..500 {
            if avatars.shared.in_flight.load(Ordering::SeqCst) == 0 {
                return;
            }
            thread::sleep(Duration::from_millis(10));
        }
        panic!("the avatar worker did not finish");
    }

    #[test]
    fn failed_downloads_retry_with_a_growing_back_off() {
        let secs = Duration::from_secs;
        assert_eq!(retry_delay(1), secs(60));
        assert_eq!(retry_delay(2), secs(120));
        assert_eq!(retry_delay(3), secs(240));
        assert_eq!(retry_delay(5), secs(900));
        assert_eq!(retry_delay(6), secs(900));
        assert_eq!(retry_delay(u32::MAX), secs(900));
        assert!(!retry_due(secs(59), 1));
        assert!(retry_due(secs(60), 1));
        assert!(!retry_due(secs(100), 2));
        assert!(retry_due(secs(120), 2));
    }

    #[test]
    fn the_no_gravatar_marker_expires_after_a_week() {
        let day = Duration::from_secs(24 * 60 * 60);
        assert!(!none_marker_expired(day * 6));
        assert!(none_marker_expired(day * 7));
        let folder = tempfile::tempdir().expect("a temporary folder");
        let marker = folder.path().join("a.png.none");
        assert!(!has_none_marker(&marker));
        fs::write(&marker, "").expect("writable");
        assert!(has_none_marker(&marker));
        let old = std::time::SystemTime::now() - day * 8;
        fs::File::options()
            .write(true)
            .open(&marker)
            .and_then(|file| file.set_modified(old))
            .expect("an old marker");
        assert!(!has_none_marker(&marker));
        assert!(!marker.exists(), "an expired marker is removed");
    }

    #[test]
    fn initials_of_names_and_addresses() {
        assert_eq!(initials(&author("Ada Lovelace", "ada@example.com")), "AL");
        assert_eq!(initials(&author("ada", "")), "A");
        assert_eq!(initials(&author("jean-luc picard", "")), "JL");
        assert_eq!(initials(&author("A. B. Cee", "")), "AB");
        assert_eq!(initials(&author("", "first.last@example.com")), "FL");
        assert_eq!(initials(&author("", "solo@example.com")), "S");
        assert_eq!(initials(&author("", "")), "?");
        assert_eq!(initials(&author("Émile Zola", "")), "Z");
        assert_eq!(initials(&author("(bot) 9lives", "")), "9");
    }

    #[test]
    fn colours_come_from_the_address() {
        assert_eq!(email_color(""), None);
        let color = email_color("Ada@Example.com").expect("a colour");
        assert_eq!(Some(color), email_color("ada@example.com"));
        assert_ne!(Some(color), email_color("bob@example.com"));
        // Saturation 0.45, value 0.62: the brightest channel is the value.
        assert!((color.r.max(color.g).max(color.b) - 0.62).abs() < 1e-6);
        assert!((color.r.min(color.g).min(color.b) - 0.62 * 0.55).abs() < 1e-6);
        assert_eq!(hsv_to_rgb(0.0, 1.0, 1.0), Color::new(1.0, 0.0, 0.0, 1.0));
        assert_eq!(hsv_to_rgb(1.0 / 3.0, 1.0, 1.0).to_hex(), 0x00FF00);
        assert_eq!(
            hsv_to_rgb(0.5, 0.0, 0.25),
            Color::new(0.25, 0.25, 0.25, 1.0)
        );
    }

    #[test]
    fn gravatar_keys() {
        // Gravatar's own example: the address is trimmed and lower-cased before hashing.
        assert_eq!(
            gravatar_hash(" MyEmailAddress@example.com \n"),
            "0bc83cb571cd1c50ba6f3e8a78ef1346"
        );
        assert_eq!(
            gravatar_url("abc", 44),
            "https://www.gravatar.com/avatar/abc?d=404&s=44"
        );
        assert_eq!(
            with_suffix(Path::new("dir/a.png"), ".none"),
            Path::new("dir/a.png.none")
        );
    }

    #[test]
    fn cached_avatars_load_without_the_network() {
        let folder = tempfile::tempdir().expect("a temporary folder");
        let cached = folder
            .path()
            .join(format!("{}-32.png", gravatar_hash("ada@example.com")));
        image::RgbaImage::from_pixel(4, 4, image::Rgba([10, 20, 30, 255]))
            .save(&cached)
            .expect("a PNG");
        // Known to have no Gravatar, and a file that is not an image.
        let none = folder.path().join(format!(
            "{}-32.png.none",
            gravatar_hash("nobody@example.com")
        ));
        fs::write(none, "").expect("writable");
        let broken = folder
            .path()
            .join(format!("{}-32.png", gravatar_hash("broken@example.com")));
        fs::write(&broken, "not a png").expect("writable");

        let mut textures = FakeTextures::default();
        let mut avatars = Avatars::new();
        assert!(!avatars.pending());
        avatars.set_cache_directory(Some(folder.path().to_owned()));
        for email in [
            "Ada@Example.com",
            "nobody@example.com",
            "broken@example.com",
        ] {
            assert_eq!(avatars.texture(&mut textures, email, 32), None);
        }
        assert_eq!(avatars.texture(&mut textures, "ada@example.com", 0), None);
        wait_for(&avatars);
        // Decoded: pending until a frame uploads it.
        assert!(avatars.pending());
        let texture = avatars.texture(&mut textures, "ada@example.com", 32);
        assert_eq!(texture, Some(TextureId::new(1)));
        assert_eq!(textures.created, [(4, 4)]);
        assert!(!avatars.pending());
        assert_eq!(
            avatars.texture(&mut textures, "nobody@example.com", 32),
            None
        );
        assert_eq!(
            avatars.texture(&mut textures, "broken@example.com", 32),
            None
        );
        assert!(!broken.exists(), "a file that does not decode is removed");

        avatars.set_enabled(false);
        assert_eq!(avatars.texture(&mut textures, "ada@example.com", 32), None);
        avatars.set_enabled(true);

        avatars.shutdown(&mut textures);
        assert_eq!(textures.destroyed, [TextureId::new(1)]);
        // Usable again after a shutdown.
        assert_eq!(avatars.texture(&mut textures, "ada@example.com", 32), None);
        wait_for(&avatars);
        assert_eq!(
            avatars.texture(&mut textures, "ada@example.com", 32),
            Some(TextureId::new(2))
        );
    }

    #[test]
    fn without_a_cache_directory_every_avatar_fails() {
        let mut textures = FakeTextures::default();
        let mut avatars = Avatars::new();
        assert_eq!(avatars.texture(&mut textures, "ada@example.com", 32), None);
        wait_for(&avatars);
        assert_eq!(avatars.texture(&mut textures, "ada@example.com", 32), None);
        assert!(textures.created.is_empty());
        assert!(!avatars.pending());
    }
}
