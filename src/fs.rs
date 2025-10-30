/// Simple in-memory filesystem
use spin::Mutex;

#[derive(Clone, Copy)]
pub struct File {
    pub name: &'static str,
    pub content: &'static str,
    pub is_dir: bool,
}

const MAX_FILES: usize = 64;

static FILES: Mutex<[Option<File>; MAX_FILES]> = Mutex::new([None; MAX_FILES]);
static FILE_COUNT: Mutex<usize> = Mutex::new(0);

/// Initialize filesystem with some default files
pub fn init() {
    add_file("/", "", true);
    add_file(
        "/README.txt",
        "Welcome to NexaOS!\n\nThis is a hybrid-kernel operating system.\n",
        false,
    );
    add_file("/hello.txt", "Hello, World!\n", false);
    add_file("/test.txt", "This is a test file.\nLine 2\nLine 3\n", false);
    add_file(
        "/about.txt",
        "NexaOS v0.0.1\nBuilt with Rust\nUser-space shell enabled\n",
        false,
    );

    crate::kinfo!("Filesystem initialized with {} files", *FILE_COUNT.lock());
}

/// Add a file to the filesystem
pub fn add_file(name: &'static str, content: &'static str, is_dir: bool) {
    let mut files = FILES.lock();
    let mut count = FILE_COUNT.lock();

    if *count < MAX_FILES {
        files[*count] = Some(File {
            name,
            content,
            is_dir,
        });
        *count += 1;
    }
}

/// List all files in root directory
pub fn list_files() -> &'static [Option<File>] {
    let files = FILES.lock();
    unsafe { core::slice::from_raw_parts(files.as_ptr(), MAX_FILES) }
}

/// Read file content
pub fn read_file(name: &str) -> Option<&'static str> {
    let files = FILES.lock();
    let count = *FILE_COUNT.lock();

    for i in 0..count {
        if let Some(file) = files[i] {
            if file.name == name && !file.is_dir {
                return Some(file.content);
            }
        }
    }
    None
}

/// Check if file exists
pub fn file_exists(name: &str) -> bool {
    let files = FILES.lock();
    let count = *FILE_COUNT.lock();

    for i in 0..count {
        if let Some(file) = files[i] {
            if file.name == name {
                return true;
            }
        }
    }
    false
}
