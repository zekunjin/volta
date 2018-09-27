use std::cell::Cell;
use std::env;
use std::fs;
use std::io::{self, ErrorKind};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering, ATOMIC_USIZE_INIT};
use std::sync::{Once, ONCE_INIT};

static INTEGRATION_TEST_DIR: &'static str = "integration_test";
static NEXT_ID: AtomicUsize = ATOMIC_USIZE_INIT;

thread_local!(static TASK_ID: usize = NEXT_ID.fetch_add(1, Ordering::SeqCst));

fn init() {
    static GLOBAL_INIT: Once = ONCE_INIT;
    thread_local!(static LOCAL_INIT: Cell<bool> = Cell::new(false));
    GLOBAL_INIT.call_once(|| {
        global_root().mkdir_p();
    });
    LOCAL_INIT.with(|i| {
        if i.get() {
            return;
        }
        i.set(true);
        root().rm_rf();
        home().mkdir_p();
    })
}

fn global_root() -> PathBuf {
    let mut path = ok_or_panic!{ env::current_exe() };
    path.pop(); // chop off exe name
    path.pop(); // chop off 'debug'

    // If `cargo test` is run manually then our path looks like
    // `target/debug/foo`, in which case our `path` is already pointing at
    // `target`. If, however, `cargo test --target $target` is used then the
    // output is `target/$target/debug/foo`, so our path is pointing at
    // `target/$target`. Here we conditionally pop the `$target` name.
    if path.file_name().and_then(|s| s.to_str()) != Some("target") {
        path.pop();
    }

    path.join(INTEGRATION_TEST_DIR)
}

pub fn root() -> PathBuf {
    init();
    global_root().join(&TASK_ID.with(|my_id| format!("t{}", my_id)))
}

pub fn home() -> PathBuf {
    root().join("home")
}

pub trait PathExt {
    fn rm_rf(&self);
    fn mkdir_p(&self);
}

impl PathExt for Path {
    /* Technically there is a potential race condition, but we don't
     * care all that much for our tests
     */
    fn rm_rf(&self) {
        if !self.exists() {
            return;
        }

        for file in ok_or_panic!{ fs::read_dir(self) } {
            let file = ok_or_panic!{ file };
            if file.file_type().map(|m| m.is_dir()).unwrap_or(false) {
                file.path().rm_rf();
            } else {
                // On windows we can't remove a readonly file, and git will
                // often clone files as readonly. As a result, we have some
                // special logic to remove readonly files on windows.
                do_op(&file.path(), "remove file", |p| fs::remove_file(p));
            }
        }
        do_op(self, "remove dir", |p| fs::remove_dir(p));
    }

    fn mkdir_p(&self) {
        fs::create_dir_all(self)
            .unwrap_or_else(|e| panic!("failed to mkdir_p {}: {}", self.display(), e))
    }
}

fn do_op<F>(path: &Path, desc: &str, mut f: F)
where
    F: FnMut(&Path) -> io::Result<()>,
{
    match f(path) {
        Ok(()) => {}
        Err(ref e) if cfg!(windows) && e.kind() == ErrorKind::PermissionDenied => {
            let mut p = ok_or_panic!{ path.metadata() }.permissions();
            p.set_readonly(false);
            ok_or_panic!{ fs::set_permissions(path, p) };
            f(path).unwrap_or_else(|e| {
                panic!("failed to {} {}: {}", desc, path.display(), e);
            })
        }
        Err(e) => {
            panic!("failed to {} {}: {}", desc, path.display(), e);
        }
    }
}
