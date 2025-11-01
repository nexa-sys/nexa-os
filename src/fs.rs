use spin::Mutex;

use crate::posix::{self, FileType, Metadata};

pub mod ext2;

#[derive(Clone, Copy)]
pub struct File {
    pub name: &'static str,
    pub content: &'static [u8],
    pub is_dir: bool,
}

const MAX_FILES: usize = 64;
const MAX_MOUNTS: usize = 8;

static FILES: Mutex<[Option<File>; MAX_FILES]> = Mutex::new([None; MAX_FILES]);
static FILE_METADATA: Mutex<[Option<Metadata>; MAX_FILES]> = Mutex::new([None; MAX_FILES]);
static FILE_COUNT: Mutex<usize> = Mutex::new(0);
static MOUNTS: Mutex<[Option<MountEntry>; MAX_MOUNTS]> = Mutex::new([None; MAX_MOUNTS]);

#[derive(Clone, Copy)]
pub enum FileContent {
    Inline(&'static [u8]),
    Ext2(ext2::FileRef),
}

#[derive(Clone, Copy)]
pub struct OpenFile {
    pub content: FileContent,
    pub metadata: Metadata,
}

pub trait FileSystem: Sync {
    fn name(&self) -> &'static str;
    fn read(&self, path: &str) -> Option<OpenFile>;
    fn metadata(&self, path: &str) -> Option<Metadata>;
    fn list(&self, path: &str, cb: &mut dyn FnMut(&'static str, Metadata));
}

#[derive(Clone, Copy)]
struct MountEntry {
    mount_point: &'static str,
    fs: &'static dyn FileSystem,
}

struct InitramfsFilesystem;

static INITFS: InitramfsFilesystem = InitramfsFilesystem;

pub fn init() {
    crate::kinfo!("Filesystem init: start");

    {
        let mut files = FILES.lock();
        let mut metas = FILE_METADATA.lock();
        for slot in files.iter_mut() {
            *slot = None;
        }
        for slot in metas.iter_mut() {
            *slot = None;
        }
        *FILE_COUNT.lock() = 0;
    }

    {
        let mut mounts = MOUNTS.lock();
        for slot in mounts.iter_mut() {
            *slot = None;
        }
    }

    mount("/", &INITFS).expect("root mount must succeed");

    if crate::initramfs::get().is_none() {
        crate::kwarn!("Filesystem init: no initramfs available; starting empty");
        return;
    }

    let mut entry_count = 0usize;
    let mut ext_candidate: Option<&'static [u8]> = None;

    crate::initramfs::for_each_entry(|entry| {
        entry_count += 1;
        let name = entry.name.strip_prefix('/').unwrap_or(entry.name);
        let (mode, file_type) = posix::split_mode(entry.mode);

        let mut meta = Metadata::empty().with_mode(mode).with_type(file_type);
        meta.size = entry.data.len() as u64;
        meta.nlink = 1;
        meta.blocks = ((meta.size + 511) / 512).max(1);

        let is_dir = matches!(file_type, FileType::Directory);

        add_file_with_metadata(name, entry.data, is_dir, meta);

        if ext_candidate.is_none()
            && matches!(file_type, FileType::Regular)
            && (name.ends_with(".ext2") || name.ends_with(".ext3") || name.ends_with(".ext4"))
        {
            ext_candidate = Some(entry.data);
        }
    });

    if let Some(image) = ext_candidate {
        match ext2::Ext2Filesystem::new(image) {
            Ok(fs) => {
                let fs_ref = ext2::register_global(fs);
                match mount("/mnt/ext", fs_ref) {
                    Ok(()) => crate::kinfo!("Mounted ext2 image at /mnt/ext"),
                    Err(err) => crate::kwarn!("Failed to mount ext2 filesystem: {:?}", err),
                }
                let mut dir_meta = Metadata::empty().with_type(FileType::Directory);
                dir_meta.nlink = 2;
                dir_meta.mode |= 0o755;
                add_file_with_metadata("mnt", &[], true, dir_meta);
                add_file_with_metadata("mnt/ext", &[], true, dir_meta);
            }
            Err(err) => {
                crate::kwarn!("Failed to parse ext2 image: {:?}", err);
            }
        }
    }

    let files_total = *FILE_COUNT.lock();
    crate::kinfo!(
        "Filesystem initialized with {} files ({} initramfs entries processed)",
        files_total,
        entry_count
    );
}

pub fn add_file(name: &'static str, content: &'static str, is_dir: bool) {
    add_file_bytes(name, content.as_bytes(), is_dir);
}

pub fn add_file_bytes(name: &'static str, content: &'static [u8], is_dir: bool) {
    let mut meta = Metadata::empty();
    meta.size = content.len() as u64;
    meta.blocks = ((meta.size + 511) / 512).max(1);
    meta.nlink = 1;
    meta = meta.with_type(if is_dir {
        FileType::Directory
    } else {
        FileType::Regular
    });
    add_file_with_metadata(name, content, is_dir, meta);
}

pub fn add_file_with_metadata(
    name: &'static str,
    content: &'static [u8],
    is_dir: bool,
    metadata: Metadata,
) {
    register_entry(
        File {
            name,
            content,
            is_dir,
        },
        metadata.normalize(),
    );
}

pub fn open(path: &str) -> Option<OpenFile> {
    let (fs, relative) = resolve_mount(path)?;
    fs.read(relative)
}

pub fn stat(path: &str) -> Option<Metadata> {
    let (fs, relative) = resolve_mount(path)?;
    fs.metadata(relative)
}

pub fn list_directory<F>(path: &str, mut cb: F)
where
    F: FnMut(&'static str, Metadata),
{
    if let Some((fs, relative)) = resolve_mount(path) {
        fs.list(relative, &mut cb);
    }
}

pub fn list_files() -> &'static [Option<File>] {
    let files = FILES.lock();
    unsafe { core::slice::from_raw_parts(files.as_ptr(), MAX_FILES) }
}

pub fn read_file_bytes(name: &str) -> Option<&'static [u8]> {
    if let Some(opened) = open(name) {
        if let FileContent::Inline(bytes) = opened.content {
            return Some(bytes);
        }
    }
    None
}

pub fn read_file(name: &str) -> Option<&'static str> {
    read_file_bytes(name).and_then(|b| core::str::from_utf8(b).ok())
}

pub fn file_exists(name: &str) -> bool {
    stat(name).is_some()
}

fn register_entry(file: File, metadata: Metadata) {
    let mut files = FILES.lock();
    let mut metas = FILE_METADATA.lock();
    let mut count = FILE_COUNT.lock();

    for idx in 0..*count {
        if let Some(existing) = files[idx] {
            if existing.name == file.name {
                files[idx] = Some(file);
                metas[idx] = Some(metadata);
                return;
            }
        }
    }

    if *count < MAX_FILES {
        files[*count] = Some(file);
        metas[*count] = Some(metadata);
        *count += 1;
    } else {
        crate::kwarn!("File table full, ignoring '{}'", file.name);
    }
}

fn mount(mount_point: &'static str, fs: &'static dyn FileSystem) -> Result<(), MountError> {
    let normalized = if mount_point == "/" {
        "/"
    } else {
        mount_point.trim_end_matches('/')
    };

    let mut mounts = MOUNTS.lock();
    if mounts
        .iter()
        .flatten()
        .any(|entry| entry.mount_point == normalized)
    {
        return Err(MountError::AlreadyMounted);
    }

    for slot in mounts.iter_mut() {
        if slot.is_none() {
            *slot = Some(MountEntry {
                mount_point: normalized,
                fs,
            });
            crate::kinfo!("Mounted {} at {}", fs.name(), normalized);
            return Ok(());
        }
    }

    Err(MountError::TableFull)
}

#[derive(Debug)]
enum MountError {
    AlreadyMounted,
    TableFull,
}

fn resolve_mount(path: &str) -> Option<(&'static dyn FileSystem, &str)> {
    if path.is_empty() {
        return None;
    }

    let is_absolute = path.starts_with('/');
    let mut best: Option<(&'static dyn FileSystem, &str, usize)> = None;
    let mounts = MOUNTS.lock();

    for entry in mounts.iter().flatten() {
        if entry.mount_point == "/" {
            let relative = if is_absolute {
                path.trim_start_matches('/')
            } else {
                path
            };
            if best.map_or(true, |(_, _, len)| len <= 1) {
                best = Some((entry.fs, relative, 1));
            }
        } else if is_absolute && path.starts_with(entry.mount_point) {
            let rest = &path[entry.mount_point.len()..];
            let relative = rest.trim_start_matches('/');
            let mp_len = entry.mount_point.len();
            if best.map_or(true, |(_, _, len)| mp_len > len) {
                best = Some((entry.fs, relative, mp_len));
            }
        }
    }

    best.map(|(fs, rel, _)| (fs, rel))
}

fn find_file_index(name: &str) -> Option<usize> {
    let files = FILES.lock();
    let count = *FILE_COUNT.lock();
    let target = name.trim_matches('/');
    for idx in 0..count {
        if let Some(file) = files[idx] {
            if file.name == target {
                return Some(idx);
            }
        }
    }
    None
}

impl FileSystem for InitramfsFilesystem {
    fn name(&self) -> &'static str {
        "initramfs"
    }

    fn read(&self, path: &str) -> Option<OpenFile> {
        let idx = find_file_index(path)?;
        let files = FILES.lock();
        let metas = FILE_METADATA.lock();
        let file = files[idx]?;
        let meta = metas[idx].unwrap_or_else(Metadata::empty);
        Some(OpenFile {
            content: FileContent::Inline(file.content),
            metadata: meta,
        })
    }

    fn metadata(&self, path: &str) -> Option<Metadata> {
        let idx = find_file_index(path)?;
        let metas = FILE_METADATA.lock();
        metas[idx]
    }

    fn list(&self, path: &str, cb: &mut dyn FnMut(&'static str, Metadata)) {
        let target = path.trim_matches('/');
        let files = FILES.lock();
        let metas = FILE_METADATA.lock();
        let count = *FILE_COUNT.lock();

        for idx in 0..count {
            if let Some(file) = files[idx] {
                let meta = metas[idx].unwrap_or_else(Metadata::empty);
                if target.is_empty() {
                    cb(file.name, meta);
                } else if file.name.starts_with(target) {
                    cb(file.name, meta);
                }
            }
        }
    }
}
