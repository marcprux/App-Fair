//! Remote image cache for list icons and screenshots. Each URL maps to one reactive
//! `Signal<Option<Arc<Vec<u8>>>>` that a `remote_image` piece renders. Loads run on a small pool
//! of background workers (not a thread per icon, which would storm the network and starve the UI),
//! reading from the on-disk cache when present. Recycled list rows asking for the same URL get the
//! same signal, so an icon is fetched at most once.

use std::cell::RefCell;
use std::collections::{HashMap, VecDeque};
use std::hash::{Hash, Hasher};
use std::path::PathBuf;
use std::sync::{Arc, Condvar, Mutex, OnceLock};

use day::prelude::*;
use day_piece_remote_image::{RemoteImage, remote_image};

/// A cached image's reactive bytes.
type ImageSig = Signal<Option<Arc<Vec<u8>>>>;

/// A `remote_image` whose bytes follow a reactive URL. As a recycling list row rebinds to a new
/// item, `url_of` yields the new icon URL and the image swaps. The URL closure MUST read the item
/// reactively (e.g. `move || slot.field(|a| a.icon_url())`).
pub fn row_icon(url_of: impl Fn() -> Option<String> + 'static) -> RemoteImage {
    let bytes: ImageSig = Signal::new(None);
    // Mirror the current URL's cached image signal into this row-local signal. Re-runs both when
    // the row rebinds (URL changes) and when that URL's bytes finish loading.
    bind(
        move || url_of().and_then(|u| image_signal(&u).get()),
        move |b: &Option<Arc<Vec<u8>>>| bytes.set(b.clone()),
    );
    remote_image(bytes)
}

/// How many icons/screenshots download at once.
const WORKERS: usize = 6;

thread_local! {
    static CACHE: RefCell<HashMap<String, ImageSig>> = RefCell::new(HashMap::new());
}

struct Job {
    url: String,
    setter: Setter<Option<Arc<Vec<u8>>>>,
}

struct Queue {
    jobs: Mutex<VecDeque<Job>>,
    ready: Condvar,
}

fn queue() -> &'static Queue {
    static Q: OnceLock<Queue> = OnceLock::new();
    Q.get_or_init(|| {
        let q = Queue {
            jobs: Mutex::new(VecDeque::new()),
            ready: Condvar::new(),
        };
        // Spawn the worker pool once, on first use.
        for _ in 0..WORKERS {
            std::thread::spawn(worker);
        }
        q
    })
}

fn worker() {
    crate::util::lower_priority();
    let q = queue();
    loop {
        let job = {
            let mut jobs = q.jobs.lock().unwrap();
            loop {
                if let Some(job) = jobs.pop_front() {
                    break job;
                }
                jobs = q.ready.wait(jobs).unwrap();
            }
        };
        load(&job.url, job.setter);
    }
}

/// The signal backing `remote_image` for `url`, enqueuing a load on first request.
pub fn image_signal(url: &str) -> ImageSig {
    CACHE.with(|c| {
        if let Some(sig) = c.borrow().get(url) {
            return *sig;
        }
        // Detached: cached image signals outlive any single list row.
        let sig = Scope::detached().enter(|| Signal::new(None::<Arc<Vec<u8>>>));
        c.borrow_mut().insert(url.to_string(), sig);
        let q = queue();
        q.jobs.lock().unwrap().push_back(Job {
            url: url.to_string(),
            setter: sig.setter(),
        });
        q.ready.notify_one();
        sig
    })
}

fn load(url: &str, setter: Setter<Option<Arc<Vec<u8>>>>) {
    let path = cache_path(url);
    if let Ok(bytes) = std::fs::read(&path)
        && !bytes.is_empty()
    {
        setter.set(Some(Arc::new(bytes)));
        return;
    }
    if let Ok(bytes) = crate::net::get_bytes(url)
        && !bytes.is_empty()
    {
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let _ = std::fs::write(&path, &bytes);
        setter.set(Some(Arc::new(bytes)));
    }
}

/// Write `bytes` into the on-disk cache under `url`, so a later request for that URL loads them
/// straight from disk without touching the network. Used to pre-warm the mock catalog's icons.
pub fn preseed(url: &str, bytes: &[u8]) {
    let path = cache_path(url);
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::write(&path, bytes);
}

fn cache_path(url: &str) -> PathBuf {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    url.hash(&mut h);
    crate::platform::data_dir()
        .join("imgcache")
        .join(format!("{:016x}", h.finish()))
}
