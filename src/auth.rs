use core::sync::atomic::{AtomicU32, Ordering};

use spin::Mutex;

const MAX_USERS: usize = 16;
const MAX_NAME_LEN: usize = 32;
const MAX_PASSWORD_LEN: usize = 64;

#[derive(Clone, Copy, Debug)]
pub struct Credentials {
    pub uid: u32,
    pub gid: u32,
    pub is_admin: bool,
}

#[derive(Clone, Copy, Debug)]
pub struct UserSummary {
    pub username: [u8; MAX_NAME_LEN],
    pub username_len: usize,
    pub uid: u32,
    pub gid: u32,
    pub is_admin: bool,
}

#[derive(Clone, Copy, Debug)]
pub struct CurrentUser {
    pub username: [u8; MAX_NAME_LEN],
    pub username_len: usize,
    pub credentials: Credentials,
}

#[derive(Clone, Copy, Debug)]
struct UserRecord {
    used: bool,
    username: [u8; MAX_NAME_LEN],
    username_len: usize,
    password_hash: u64,
    uid: u32,
    gid: u32,
    is_admin: bool,
}

impl UserRecord {
    const fn unused() -> Self {
        Self {
            used: false,
            username: [0; MAX_NAME_LEN],
            username_len: 0,
            password_hash: 0,
            uid: 0,
            gid: 0,
            is_admin: false,
        }
    }

    fn username_str(&self) -> &str {
        core::str::from_utf8(&self.username[..self.username_len]).unwrap_or("")
    }
}

#[derive(Debug)]
pub enum AuthError {
    InvalidInput,
    AlreadyExists,
    TableFull,
    InvalidCredentials,
    AccessDenied,
}

static USERS: Mutex<[UserRecord; MAX_USERS]> = Mutex::new([UserRecord::unused(); MAX_USERS]);
static CURRENT: Mutex<CurrentUser> = Mutex::new(CurrentUser {
    username: [0; MAX_NAME_LEN],
    username_len: 0,
    credentials: Credentials {
        uid: 0,
        gid: 0,
        is_admin: false,
    },
});
static NEXT_UID: AtomicU32 = AtomicU32::new(1);

pub fn init() {
    let mut users = USERS.lock();
    let mut current = CURRENT.lock();
    for slot in users.iter_mut() {
        *slot = UserRecord::unused();
    }

    let root_hash = hash_password(b"root");
    users[0] = UserRecord {
        used: true,
        username: to_array(b"root"),
        username_len: 4,
        password_hash: root_hash,
        uid: 0,
        gid: 0,
        is_admin: true,
    };

    current.username = to_array(b"root");
    current.username_len = 4;
    current.credentials = Credentials {
        uid: 0,
        gid: 0,
        is_admin: true,
    };

    NEXT_UID.store(1, Ordering::SeqCst);
    crate::kinfo!("Auth subsystem initialized with default root user");
}

pub fn current_user() -> CurrentUser {
    *CURRENT.lock()
}

pub fn enumerate_users<F>(mut f: F)
where
    F: FnMut(UserSummary),
{
    let users = USERS.lock();
    for record in users.iter() {
        if record.used {
            f(UserSummary {
                username: record.username,
                username_len: record.username_len,
                uid: record.uid,
                gid: record.gid,
                is_admin: record.is_admin,
            });
        }
    }
}

pub fn authenticate(username: &str, password: &str) -> Result<Credentials, AuthError> {
    if username.is_empty() || password.is_empty() {
        return Err(AuthError::InvalidInput);
    }

    let hash = hash_password(password.as_bytes());
    let users = USERS.lock();
    for record in users.iter() {
        if record.used && record.username_str() == username && record.password_hash == hash {
            let creds = Credentials {
                uid: record.uid,
                gid: record.gid,
                is_admin: record.is_admin,
            };
            let mut current = CURRENT.lock();
            current.username[..record.username_len]
                .copy_from_slice(&record.username[..record.username_len]);
            current.username_len = record.username_len;
            current.credentials = creds;
            drop(current);
            return Ok(creds);
        }
    }
    Err(AuthError::InvalidCredentials)
}

pub fn create_user(username: &str, password: &str, is_admin: bool) -> Result<u32, AuthError> {
    if username.is_empty() || password.is_empty() {
        return Err(AuthError::InvalidInput);
    }
    if username.len() > MAX_NAME_LEN || password.len() > MAX_PASSWORD_LEN {
        return Err(AuthError::InvalidInput);
    }

    let mut users = USERS.lock();
    if users.iter().any(|u| u.used && u.username_str() == username) {
        return Err(AuthError::AlreadyExists);
    }

    if let Some(slot) = users.iter_mut().find(|u| !u.used) {
        let uid = NEXT_UID.fetch_add(1, Ordering::SeqCst);
        slot.used = true;
        slot.username = to_array(username.as_bytes());
        slot.username_len = username.len();
        slot.password_hash = hash_password(password.as_bytes());
        slot.uid = uid;
        slot.gid = uid;
        slot.is_admin = is_admin;
        crate::kinfo!("Created user '{}' (uid={})", username, uid);
        Ok(uid)
    } else {
        Err(AuthError::TableFull)
    }
}

pub fn require_admin() -> bool {
    CURRENT.lock().credentials.is_admin
}

/// Check if current user is superuser (UID 0 or admin flag)
pub fn is_superuser() -> bool {
    let current = CURRENT.lock();
    current.credentials.uid == 0 || current.credentials.is_admin
}

/// Get current user's UID
pub fn current_uid() -> u32 {
    CURRENT.lock().credentials.uid
}

/// Get current user's GID
pub fn current_gid() -> u32 {
    CURRENT.lock().credentials.gid
}

pub fn logout() -> Result<(), AuthError> {
    let root_record = {
        let users = USERS.lock();
        users
            .iter()
            .find(|record| record.used && record.uid == 0)
            .copied()
    };

    if let Some(root) = root_record {
        let mut current = CURRENT.lock();
        current.username.iter_mut().for_each(|byte| *byte = 0);
        current.username[..root.username_len].copy_from_slice(&root.username[..root.username_len]);
        current.username_len = root.username_len;
        current.credentials = Credentials {
            uid: root.uid,
            gid: root.gid,
            is_admin: root.is_admin,
        };
        Ok(())
    } else {
        Err(AuthError::InvalidInput)
    }
}

fn to_array(bytes: &[u8]) -> [u8; MAX_NAME_LEN] {
    let mut arr = [0u8; MAX_NAME_LEN];
    let len = core::cmp::min(bytes.len(), MAX_NAME_LEN);
    arr[..len].copy_from_slice(&bytes[..len]);
    arr
}

fn hash_password(bytes: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf29ce484222325;
    for &b in bytes {
        hash ^= b as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

impl UserSummary {
    pub fn username_str(&self) -> &str {
        core::str::from_utf8(&self.username[..self.username_len]).unwrap_or("")
    }
}
