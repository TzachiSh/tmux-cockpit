//! The storage contract (#1020): a key-value store with revisions behind one
//! trait, so a NATS JetStream KV backend can replace [`LocalStore`] later
//! without changing callers. Nothing calls it yet. contracts/README.md maps
//! each method to NATS KV and says what [`Store::watch`] promises.
use serde_json::Value;
use std::fs::{DirBuilder, File, OpenOptions};
use std::io::{self, Read, Write};
use std::os::fd::AsRawFd;
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, mpsc};
use std::time::{Duration, Instant};

/// Longest key in bytes: with `~lock` or `~tmp` it stays under a file name's 255.
pub const KEY_MAX: usize = 200;
/// Largest store file. A put whose file would be larger is refused, since a
/// read refuses one too.
pub const FILE_MAX: u64 = 64 * 1024;
const HEAD: &str = "cockpit-store 1";

/// Dotted tokens, each non-empty `[A-Za-z0-9_-]`: a NATS subject with no
/// wildcard, and a file name with no `/` and no leading dot.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Key(String);

/// A refused key and why.
#[derive(Debug, PartialEq, Eq)]
pub struct KeyError {
    pub key: String,
    pub why: &'static str,
}

impl std::fmt::Display for KeyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "store key {:?}: {}", self.key, self.why)
    }
}

impl std::error::Error for KeyError {}

impl Key {
    pub fn new(key: &str) -> Result<Key, KeyError> {
        let bad = |why| Err(KeyError { key: key.to_string(), why });
        let token = |t: &str| !t.is_empty() && t.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-');
        if key.len() > KEY_MAX {
            return bad("longer than 200 bytes");
        }
        if !key.split('.').all(token) {
            return bad("not dotted tokens of [A-Za-z0-9_-]");
        }
        Ok(Key(key.to_string()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Strictly below `prefix`, as NATS's `prefix.>` matches.
    pub fn is_under(&self, prefix: &Key) -> bool {
        self.0.strip_prefix(&prefix.0).is_some_and(|rest| rest.starts_with('.'))
    }
}

impl std::fmt::Display for Key {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// Compared for equality only: a NATS revision is the stream's sequence and
/// skips, while [`LocalStore`] counts 1, 2, 3 per key.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Revision(pub u64);

#[derive(Debug)]
pub enum PutError {
    /// The key's revision is not the one expected; `None` when it has no value.
    Conflict {
        current: Option<Revision>,
    },
    /// Another writer held the key past the wait; try again.
    Busy,
    /// The value would not read back: its file passes [`FILE_MAX`].
    TooLarge,
    Io(io::Error),
}

impl std::fmt::Display for PutError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PutError::Conflict { current: Some(r) } => write!(f, "conflict: the key is at revision {}", r.0),
            PutError::Conflict { current: None } => write!(f, "conflict: the key has no value"),
            PutError::Busy => write!(f, "busy: another writer holds the key"),
            PutError::TooLarge => write!(f, "too large: the value's file passes {FILE_MAX} bytes"),
            PutError::Io(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for PutError {}

impl From<io::Error> for PutError {
    fn from(e: io::Error) -> Self {
        PutError::Io(e)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Update {
    pub key: Key,
    pub value: Value,
    pub revision: Revision,
}

/// The receiving end of [`Store::watch`]; dropping it stops the watch.
pub struct Watch {
    rx: mpsc::Receiver<Update>,
    stop: Arc<AtomicBool>,
}

impl Watch {
    /// For a backend: its sender feeds `rx`, and it stops once `stop` is set.
    pub fn new(rx: mpsc::Receiver<Update>, stop: Arc<AtomicBool>) -> Watch {
        Watch { rx, stop }
    }

    pub fn try_recv(&self) -> Option<Update> {
        self.rx.try_recv().ok()
    }

    pub fn recv_timeout(&self, wait: Duration) -> Option<Update> {
        self.rx.recv_timeout(wait).ok()
    }
}

impl Drop for Watch {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

/// Callers hold this trait, never a backend. Values are the JSON contracts/
/// describes, checked by no one at run time.
pub trait Store: Send + Sync {
    /// `None` for a missing key and for one whose stored form cannot be read.
    fn get(&self, key: &Key) -> Option<(Value, Revision)>;
    /// `expected: None` creates only; `Some(r)` replaces only revision `r`.
    fn put_if_revision(&self, key: &Key, value: &Value, expected: Option<Revision>) -> Result<Revision, PutError>;
    /// The latest value of each key under `prefix`, then later changes.
    fn watch(&self, prefix: &Key) -> Watch;
}

/// `$XDG_STATE_HOME/tmux-cockpit/store`, else `$HOME/.local/state/...`. A
/// relative value is ignored, as the XDG spec says; `None` without either.
pub fn root_from(state_home: Option<&str>, home: Option<&str>) -> Option<PathBuf> {
    let base = match (state_home, home) {
        (Some(s), _) if s.starts_with('/') => PathBuf::from(s),
        (_, Some(h)) if h.starts_with('/') => Path::new(h).join(".local/state"),
        _ => return None,
    };
    Some(base.join("tmux-cockpit/store"))
}

/// One file per key under a 0700 folder: a version line, `rev <n>`, the value
/// as one line of JSON. Beside it `<key>~lock` and, mid-write, `<key>~tmp`.
#[derive(Clone, Debug)]
pub struct LocalStore {
    root: PathBuf,
    lock_wait: Duration,
    poll: Duration,
}

impl LocalStore {
    /// Refuses a relative root, which would land in the cwd.
    pub fn new(root: impl Into<PathBuf>) -> io::Result<LocalStore> {
        let root = root.into();
        if !root.is_absolute() {
            let msg = format!("store root must be absolute: {}", root.display());
            return Err(io::Error::new(io::ErrorKind::InvalidInput, msg));
        }
        Ok(LocalStore {
            root,
            lock_wait: Duration::from_secs(1),
            poll: Duration::from_millis(250),
        })
    }

    /// At [`root_from`] the environment's XDG_STATE_HOME and HOME.
    pub fn at_default_root() -> Option<LocalStore> {
        let var = |k| std::env::var(k).ok();
        LocalStore::new(root_from(var("XDG_STATE_HOME").as_deref(), var("HOME").as_deref())?).ok()
    }

    /// How long a put waits for another writer's lock before [`PutError::Busy`].
    pub fn with_lock_wait(self, lock_wait: Duration) -> LocalStore {
        LocalStore { lock_wait, ..self }
    }

    /// How often a watch reads its keys again.
    pub fn with_poll(self, poll: Duration) -> LocalStore {
        LocalStore { poll, ..self }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn path(&self, key: &Key) -> PathBuf {
        self.root.join(&key.0)
    }

    /// `~` is in no key, so these names never collide with one.
    pub fn lock_path(&self, key: &Key) -> PathBuf {
        self.root.join(format!("{}~lock", key.0))
    }

    pub fn temp_path(&self, key: &Key) -> PathBuf {
        self.root.join(format!("{}~tmp", key.0))
    }

    /// The key's lock, taken with LOCK_NB and retried until the wait runs out:
    /// a writer that hangs holding it must not hang every other one.
    fn lock(&self, key: &Key) -> Result<File, PutError> {
        let f = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
            .open(self.lock_path(key))?;
        if !f.metadata()?.is_file() {
            return Err(io::Error::other("the lock is not a regular file").into());
        }
        let end = Instant::now() + self.lock_wait;
        loop {
            // SAFETY: flock on an fd this function owns.
            if unsafe { libc::flock(f.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0 {
                return Ok(f);
            }
            let e = io::Error::last_os_error();
            if e.raw_os_error() != Some(libc::EWOULDBLOCK) && e.kind() != io::ErrorKind::Interrupted {
                return Err(e.into());
            }
            if Instant::now() >= end {
                return Err(PutError::Busy);
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    /// Under the key's lock only, so `~tmp` has one writer: a killed writer's
    /// leftover is removed first. 0600 is passed to the create, which a umask
    /// can only narrow; fsync, then a rename the old file is replaced by whole.
    fn replace(&self, key: &Key, text: &str) -> io::Result<()> {
        let tmp = self.temp_path(key);
        match std::fs::remove_file(&tmp) {
            Err(e) if e.kind() != io::ErrorKind::NotFound => return Err(e),
            _ => {}
        }
        let written = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW)
            .open(&tmp)
            .and_then(|mut f| f.write_all(text.as_bytes()).and_then(|_| f.sync_all()))
            .and_then(|_| std::fs::rename(&tmp, self.path(key)));
        if written.is_err() {
            let _ = std::fs::remove_file(&tmp);
        }
        written?;
        // The rename reaches the disk with the folder's own fsync.
        File::open(&self.root).and_then(|d| d.sync_all())
    }
}

/// The stored form: strict, so anything else reads as no value.
fn parse(body: &str) -> Option<(Value, Revision)> {
    let mut lines = body.strip_suffix('\n')?.split('\n');
    let (head, rev, json) = (lines.next()?, lines.next()?, lines.next()?);
    if head != HEAD || lines.next().is_some() {
        return None;
    }
    // Digits only: u64's parse takes a leading '+'.
    let rev = rev
        .strip_prefix("rev ")
        .filter(|r| !r.is_empty() && r.bytes().all(|b| b.is_ascii_digit()))?;
    let rev = rev.parse().ok().filter(|r| *r > 0)?;
    Some((serde_json::from_str(json).ok()?, Revision(rev)))
}

fn render(value: &Value, rev: Revision) -> String {
    format!("{HEAD}\nrev {}\n{value}\n", rev.0)
}

/// A small regular file only. O_NOFOLLOW refuses a symlink and O_NONBLOCK
/// opens a FIFO without waiting for a writer; the open file is then checked.
fn read_entry(path: &Path) -> Option<(Value, Revision)> {
    let small = |m: std::fs::Metadata| m.is_file() && m.len() <= FILE_MAX;
    let f = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)
        .ok()?;
    if !f.metadata().is_ok_and(small) {
        return None;
    }
    let mut body = String::new();
    f.take(FILE_MAX).read_to_string(&mut body).ok()?;
    parse(&body)
}

/// One pass over the folder: each key under `prefix` whose revision differs
/// from the one last sent. `false` once the receiver is gone.
fn scan(root: &Path, prefix: &Key, sent: &mut std::collections::HashMap<Key, Revision>, tx: &mpsc::Sender<Update>) -> bool {
    let Ok(dir) = std::fs::read_dir(root) else { return true };
    for entry in dir.flatten() {
        let Some(key) = entry.file_name().to_str().and_then(|n| Key::new(n).ok()) else {
            continue;
        };
        if !key.is_under(prefix) {
            continue;
        }
        let Some((value, revision)) = read_entry(&entry.path()) else {
            continue;
        };
        if sent.get(&key) == Some(&revision) {
            continue;
        }
        sent.insert(key.clone(), revision);
        if tx.send(Update { key, value, revision }).is_err() {
            return false;
        }
    }
    true
}

impl Store for LocalStore {
    fn get(&self, key: &Key) -> Option<(Value, Revision)> {
        read_entry(&self.path(key))
    }

    fn put_if_revision(&self, key: &Key, value: &Value, expected: Option<Revision>) -> Result<Revision, PutError> {
        DirBuilder::new().recursive(true).mode(0o700).create(&self.root)?;
        let _lock = self.lock(key)?;
        let current = read_entry(&self.path(key)).map(|(_, r)| r);
        if current != expected {
            return Err(PutError::Conflict { current });
        }
        let next = match current {
            None => Revision(1),
            Some(r) => Revision(r.0.checked_add(1).ok_or_else(|| io::Error::other("revision at u64::MAX"))?),
        };
        let text = render(value, next);
        if text.len() as u64 > FILE_MAX {
            return Err(PutError::TooLarge);
        }
        self.replace(key, &text)?;
        Ok(next)
    }

    /// Polls: reads every key under `prefix` each [`LocalStore::with_poll`]
    /// and sends one whose revision changed. The first batch is read before
    /// this returns. No history, no order across keys, no deletes.
    fn watch(&self, prefix: &Key) -> Watch {
        let (tx, rx) = mpsc::channel();
        let stop = Arc::new(AtomicBool::new(false));
        let mut sent = std::collections::HashMap::new();
        scan(&self.root, prefix, &mut sent, &tx);
        let (root, prefix, poll, flag) = (self.root.clone(), prefix.clone(), self.poll, stop.clone());
        std::thread::spawn(move || {
            while !flag.load(Ordering::Relaxed) {
                std::thread::sleep(poll);
                if !scan(&root, &prefix, &mut sent, &tx) {
                    break;
                }
            }
        });
        Watch::new(rx, stop)
    }
}
