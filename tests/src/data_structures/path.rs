//! Path manipulation utilities
//!
//! Used for filesystem path operations.

/// Normalize a path by removing . and .. components
pub fn normalize_path(path: &str) -> String {
    let mut components: Vec<&str> = Vec::new();
    let is_absolute = path.starts_with('/');
    
    for component in path.split('/') {
        match component {
            "" | "." => continue,
            ".." => {
                if !components.is_empty() && components.last() != Some(&"..") {
                    components.pop();
                } else if !is_absolute {
                    components.push("..");
                }
            }
            _ => components.push(component),
        }
    }
    
    let result = components.join("/");
    if is_absolute {
        format!("/{}", result)
    } else if result.is_empty() {
        ".".to_string()
    } else {
        result
    }
}

/// Join two paths
pub fn join_paths(base: &str, path: &str) -> String {
    if path.starts_with('/') {
        return path.to_string();
    }
    
    let base = base.trim_end_matches('/');
    if base.is_empty() {
        path.to_string()
    } else {
        format!("{}/{}", base, path)
    }
}

/// Get the parent directory of a path
pub fn parent(path: &str) -> Option<String> {
    let path = path.trim_end_matches('/');
    if path.is_empty() || path == "/" {
        return None;
    }
    
    match path.rfind('/') {
        Some(0) => Some("/".to_string()),
        Some(idx) => Some(path[..idx].to_string()),
        None => Some(".".to_string()),
    }
}

/// Get the filename component of a path
pub fn filename(path: &str) -> Option<&str> {
    let path = path.trim_end_matches('/');
    if path.is_empty() {
        return None;
    }
    
    match path.rfind('/') {
        Some(idx) => {
            let name = &path[idx + 1..];
            if name.is_empty() { None } else { Some(name) }
        }
        None => Some(path),
    }
}

/// Get the extension of a filename
pub fn extension(path: &str) -> Option<&str> {
    filename(path).and_then(|name| {
        match name.rfind('.') {
            Some(idx) if idx > 0 => Some(&name[idx + 1..]),
            _ => None,
        }
    })
}

/// Check if a path is absolute
pub fn is_absolute(path: &str) -> bool {
    path.starts_with('/')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize_absolute() {
        assert_eq!(normalize_path("/a/b/c"), "/a/b/c");
        assert_eq!(normalize_path("/a/./b/c"), "/a/b/c");
        assert_eq!(normalize_path("/a/b/../c"), "/a/c");
        assert_eq!(normalize_path("/a/b/../../c"), "/c");
        assert_eq!(normalize_path("/a/b/../../../c"), "/c");
        assert_eq!(normalize_path("///a//b///c///"), "/a/b/c");
    }

    #[test]
    fn test_normalize_relative() {
        assert_eq!(normalize_path("a/b/c"), "a/b/c");
        assert_eq!(normalize_path("a/./b/c"), "a/b/c");
        assert_eq!(normalize_path("a/b/../c"), "a/c");
        assert_eq!(normalize_path("a/b/../../c"), "c");
        assert_eq!(normalize_path("../a/b"), "../a/b");
        assert_eq!(normalize_path("a/../.."), "..");
    }

    #[test]
    fn test_normalize_edge_cases() {
        assert_eq!(normalize_path("/"), "/");
        assert_eq!(normalize_path("."), ".");
        assert_eq!(normalize_path(".."), "..");
        assert_eq!(normalize_path("./"), ".");
        assert_eq!(normalize_path(""), ".");
    }

    #[test]
    fn test_join_paths() {
        assert_eq!(join_paths("/home", "user"), "/home/user");
        assert_eq!(join_paths("/home/", "user"), "/home/user");
        assert_eq!(join_paths("/home", "/etc"), "/etc");
        assert_eq!(join_paths("", "foo"), "foo");
        assert_eq!(join_paths("base", "sub/dir"), "base/sub/dir");
    }

    #[test]
    fn test_parent() {
        assert_eq!(parent("/home/user"), Some("/home".to_string()));
        assert_eq!(parent("/home"), Some("/".to_string()));
        assert_eq!(parent("/"), None);
        assert_eq!(parent("foo/bar"), Some("foo".to_string()));
        assert_eq!(parent("foo"), Some(".".to_string()));
        assert_eq!(parent(""), None);
    }

    #[test]
    fn test_filename() {
        assert_eq!(filename("/home/user/file.txt"), Some("file.txt"));
        assert_eq!(filename("/home/user/"), Some("user"));
        assert_eq!(filename("file.txt"), Some("file.txt"));
        assert_eq!(filename("/"), None);
        assert_eq!(filename(""), None);
    }

    #[test]
    fn test_extension() {
        assert_eq!(extension("file.txt"), Some("txt"));
        assert_eq!(extension("file.tar.gz"), Some("gz"));
        assert_eq!(extension("file"), None);
        assert_eq!(extension(".hidden"), None);
        assert_eq!(extension("/path/to/file.rs"), Some("rs"));
    }

    #[test]
    fn test_is_absolute() {
        assert!(is_absolute("/"));
        assert!(is_absolute("/home"));
        assert!(is_absolute("/home/user"));
        assert!(!is_absolute("home"));
        assert!(!is_absolute("./home"));
        assert!(!is_absolute("../home"));
    }
}
