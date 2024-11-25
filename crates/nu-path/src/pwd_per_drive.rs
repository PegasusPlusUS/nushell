use std::collections::HashMap;
/// Usage for pwd_per_drive on windows
///
/// Upon change PWD, call set_pwd() with absolute path
///
/// Call expand_pwd() with relative path to get absolution path
///
/// ```
/// use std::path::{Path, PathBuf};
/// use nu_path::{expand_pwd, set_pwd};
///
/// // Set PWD for drive C
/// set_pwd(Path::new(r"C:\Users\Home")).unwrap();
///
/// // Expand a relative path
/// let expanded = expand_pwd(Path::new("c:test"));
/// assert_eq!(expanded, Some(PathBuf::from(r"C:\Users\Home\test")));
///
/// // Will NOT expand an absolute path
/// let expanded = expand_pwd(Path::new(r"C:\absolute\path"));
/// assert_eq!(expanded, None);
///
/// // Expand with no drive letter
/// let expanded = expand_pwd(Path::new(r"\no_drive"));
/// assert_eq!(expanded, None);
///
/// // Expand with no PWD set for the drive
/// let expanded = expand_pwd(Path::new("D:test"));
/// assert!(expanded.is_some());
/// let abs_path = expanded.unwrap().as_path().to_str().expect("OK").to_string();
/// assert!(abs_path.starts_with(r"D:\"));
/// assert!(abs_path.ends_with(r"\test"));
/// ```
use std::path::{Path, PathBuf};

#[derive(PartialEq, Debug)]
pub enum PathError {
    InvalidDriveLetter,
    InvalidPath,
    CantLockSharedMap,
}
pub mod shared_map {
    use super::*;
    use crate::pwd_per_drive::PathError::CantLockSharedMap;

    /// set_pwd_per_drive
    /// Record PWD for drive, path must be absolute path
    /// return Ok(()) if succeeded, otherwise error message
    pub fn set_pwd(path: &Path) -> Result<(), PathError> {
        if let Ok(mut pwd_per_drive) = get_drive_pwd_map().lock() {
            pwd_per_drive.set_pwd(path)
        } else {
            Err(CantLockSharedMap)
        }
    }

    /// expand_pwe_per_drive
    /// Input relative path, expand PWD-per-drive to construct absolute path
    /// return PathBuf for absolute path, None if input path is invalid.
    pub fn expand_pwd(path: &Path) -> Option<PathBuf> {
        if need_expand(path) {
            if let Ok(mut pwd_per_drive) = get_drive_pwd_map().lock() {
                return pwd_per_drive.expand_path(path);
            }
        }
        None
    }
}

/// Helper to check if input path is relative path
/// with drive letter, it can be expanded with PWD-per-drive.
fn need_expand(path: &Path) -> bool {
    if let Some(path_str) = path.to_str() {
        let chars: Vec<char> = path_str.chars().collect();
        if chars.len() >= 2 {
            return chars[1] == ':' && (chars.len() == 2 || (chars[2] != '/' && chars[2] != '\\'));
        }
    }
    false
}

struct DriveToPwdMap {
    map: [Option<String>; 26], // Fixed-size array for A-Z
}

impl DriveToPwdMap {
    pub fn new() -> Self {
        // Initialize by current PWD-per-drive
        let mut map: [Option<String>; 26] = Default::default();
        for (drive_index, drive_letter) in ('A'..='Z').enumerate() {
            if let Some(pwd) = get_full_path_name_w(&format!("{}:", drive_letter)) {
                if pwd.len() > 3 {
                    map[drive_index] = Some(pwd.clone());
                }
            }
        }
        Self { map }
    }

    pub fn get_env_for_child_process(&self, env: &mut HashMap<String, String>) {
        for (drive_index, drive_letter) in ('A'..='Z').enumerate() {
            if let Some(pwd) = self.map[drive_index].clone() {
                env.insert(format!("={}:", drive_letter), pwd);
            }
        }
    }

    /// Set the PWD for the drive letter in the absolute path.
    /// Return String for error description.
    pub fn set_pwd(&mut self, path: &Path) -> Result<(), PathError> {
        if let (Some(drive_letter), Some(path_str)) =
            (Self::extract_drive_letter(path), path.to_str())
        {
            if drive_letter.is_ascii_alphabetic() {
                let drive_letter = drive_letter.to_ascii_uppercase();
                // Make sure saved drive letter is upper case
                let mut c = path_str.chars();
                match c.next() {
                    None => Err(PathError::InvalidDriveLetter),
                    Some(_) => {
                        let drive_index = drive_letter as usize - 'A' as usize;
                        self.map[drive_index] = Some(drive_letter.to_string() + c.as_str());
                        let _ = std::env::set_current_dir(path_str);
                        Ok(())
                    }
                }
            } else {
                Err(PathError::InvalidDriveLetter)
            }
        } else {
            Err(PathError::InvalidPath)
        }
    }

    /// Get the PWD for drive, if not yet, ask GetFullPathNameW(),
    /// or else return default r"X:\".
    fn get_pwd(&mut self, drive_letter: char) -> Result<String, PathError> {
        if drive_letter.is_ascii_alphabetic() {
            let drive_letter = drive_letter.to_ascii_uppercase();
            let drive_index = drive_letter as usize - 'A' as usize;
            Ok(self.map[drive_index].clone().unwrap_or_else(|| {
                if let Some(sys_pwd) = get_full_path_name_w(&format!("{}:", drive_letter)) {
                    self.map[drive_index] = Some(sys_pwd.clone());
                    sys_pwd
                } else {
                    format!(r"{}:\", drive_letter)
                }
            }))
        } else {
            Err(PathError::InvalidDriveLetter)
        }
    }

    /// Expand a relative path using the PWD-per-drive, return PathBuf
    /// of absolute path.
    /// Return None if path is not valid or can't get drive letter.
    pub fn expand_path(&mut self, path: &Path) -> Option<PathBuf> {
        if need_expand(path) {
            let path_str = path.to_str()?;
            if let Some(drive_letter) = Self::extract_drive_letter(path) {
                if let Ok(pwd) = self.get_pwd(drive_letter) {
                    // Combine current PWD with the relative path
                    let mut base = PathBuf::from(Self::ensure_trailing_delimiter(&pwd));
                    // need_expand() and extract_drive_letter() all ensure path_str.len() >= 2
                    base.push(&path_str[2..]); // Join PWD with path parts after "C:"
                    return Some(base);
                }
            }
        }
        None // Invalid path or has no drive letter
    }

    /// Extract the drive letter from a path (e.g., `C:test` -> `C`)
    fn extract_drive_letter(path: &Path) -> Option<char> {
        path.to_str()
            .and_then(|s| s.chars().next())
            .filter(|c| c.is_ascii_alphabetic())
    }

    /// Ensure a path has a trailing `\`
    fn ensure_trailing_delimiter(path: &str) -> String {
        if !path.ends_with('\\') && !path.ends_with('/') {
            format!(r"{}\", path)
        } else {
            path.to_string()
        }
    }
}

fn get_full_path_name_w(path_str: &str) -> Option<String> {
    use omnipath::sys_absolute;
    if let Ok(path_sys_abs) = sys_absolute(PathBuf::from(path_str).as_path()) {
        Some(path_sys_abs.to_str()?.to_string())
    } else {
        None
    }
}

use std::sync::{Mutex, OnceLock};

/// Global shared_map instance of DrivePwdMap
static DRIVE_PWD_MAP: OnceLock<Mutex<DriveToPwdMap>> = OnceLock::new();

/// Access the shared_map instance
fn get_drive_pwd_map() -> &'static Mutex<DriveToPwdMap> {
    DRIVE_PWD_MAP.get_or_init(|| Mutex::new(DriveToPwdMap::new()))
}

/// Test for Drive2PWD map
#[cfg(test)]
mod tests {
    use super::*;

    /// Test or demo usage of PWD-per-drive
    /// In doctest, there's no get_full_path_name_w available so can't foresee
    /// possible result, here can have more accurate test assert
    #[test]
    fn test_usage_for_pwd_per_drive() {
        use shared_map::{expand_pwd, set_pwd};
        // Set PWD for drive F
        assert!(set_pwd(Path::new(r"F:\Users\Home")).is_ok());

        // Expand a relative path
        let expanded = expand_pwd(Path::new("f:test"));
        assert_eq!(expanded, Some(PathBuf::from(r"F:\Users\Home\test")));

        // Will NOT expand an absolute path
        let expanded = expand_pwd(Path::new(r"F:\absolute\path"));
        assert_eq!(expanded, None);

        // Expand with no drive letter
        let expanded = expand_pwd(Path::new(r"\no_drive"));
        assert_eq!(expanded, None);

        // Expand with no PWD set for the drive
        let expanded = expand_pwd(Path::new("D:test"));
        if let Some(sys_abs) = get_full_path_name_w("D:") {
            assert_eq!(
                expanded,
                Some(PathBuf::from(format!(
                    "{}test",
                    DriveToPwdMap::ensure_trailing_delimiter(&sys_abs)
                )))
            );
        } else {
            assert_eq!(expanded, Some(PathBuf::from(r"D:\test")));
        }
    }

    #[test]
    fn test_usage_for_prepare_environment_for_child_process() {
        let mut map = DriveToPwdMap::new();
        map.set_pwd(&Path::new(r"C:\Home")).unwrap();
        map.set_pwd(&Path::new(r"D:\User")).unwrap();
        map.set_pwd(&Path::new(r"E:\Etc")).unwrap();
        map.set_pwd(&Path::new(r"X:\Shared")).unwrap();
        map.set_pwd(&Path::new(r"y:\Sys")).unwrap();
        map.set_pwd(&Path::new(r"z:\Bin")).unwrap();

        let mut env = HashMap::<String, String>::new();
        map.get_env_for_child_process(&mut env);
        assert_eq!(env.get("=C:").unwrap(), r"C:\Home");
        assert_eq!(env.get("=D:").unwrap(), r"D:\User");
        assert_eq!(env.get("=E:").unwrap(), r"E:\Etc");
        assert_eq!(env.get("=X:").unwrap(), r"X:\Shared");
        assert_eq!(env.get("=Y:").unwrap(), r"Y:\Sys");
        assert_eq!(env.get("=Z:").unwrap(), r"Z:\Bin");
    }

    #[test]
    fn test_shared_set_and_get_pwd() {
        // To avoid conflict with other test threads (on testing result),
        // use different drive set in multiple shared_map tests
        let drive_pwd_map = get_drive_pwd_map();
        {
            let mut map = drive_pwd_map.lock().unwrap();

            // Set PWD for drive X
            assert!(map.set_pwd(Path::new(r"X:\Users\Example")).is_ok());
        }

        {
            let mut map = drive_pwd_map.lock().unwrap();

            // Get PWD for drive X
            assert_eq!(map.get_pwd('X'), Ok(r"X:\Users\Example".to_string()));

            // Get PWD for drive E (not set, should return E:\) ???
            // 11-21-2024 happened to start nushell from drive E:,
            // run toolkit test 'toolkit check pr' then this test failed
            // since for drive that has not bind PWD, if the drive really exists
            // in system and current directory is not drive root, this test will
            // fail if assuming result should be r"X:\", and there might also have
            // other cases tested by other threads which might change PWD.
            if let Some(pwd_on_e) = get_full_path_name_w("E:") {
                assert_eq!(map.get_pwd('E'), Ok(pwd_on_e));
            } else {
                assert_eq!(map.get_pwd('E'), Ok(r"E:\".to_string()));
            }
        }
    }

    #[test]
    fn test_expand_path() {
        let mut drive_map = DriveToPwdMap::new();

        // Set PWD for drive 'C:'
        assert_eq!(drive_map.set_pwd(Path::new(r"C:\Users")), Ok(()));
        // or 'c:'
        assert_eq!(drive_map.set_pwd(Path::new(r"c:\Users\Home")), Ok(()));

        // Expand a relative path on 'C:'
        let expanded = drive_map.expand_path(Path::new(r"C:test"));
        assert_eq!(expanded, Some(PathBuf::from(r"C:\Users\Home\test")));
        // or on 'c:'
        let expanded = drive_map.expand_path(Path::new(r"c:test"));
        assert_eq!(expanded, Some(PathBuf::from(r"C:\Users\Home\test")));

        // Expand an absolute path
        let expanded = drive_map.expand_path(Path::new(r"C:\absolute\path"));
        assert_eq!(expanded, None);

        // Expand with no drive letter
        let expanded = drive_map.expand_path(Path::new(r"\no_drive"));
        assert_eq!(expanded, None);

        // Expand with no PWD set for the drive
        let expanded = drive_map.expand_path(Path::new("D:test"));
        if let Some(pwd_on_d) = get_full_path_name_w("D:") {
            assert_eq!(
                expanded,
                Some(PathBuf::from(format!(
                    r"{}test",
                    DriveToPwdMap::ensure_trailing_delimiter(&pwd_on_d)
                )))
            );
        } else {
            assert_eq!(expanded, Some(PathBuf::from(r"D:\test")));
        }
    }

    #[test]
    fn test_set_and_get_pwd() {
        let mut drive_map = DriveToPwdMap::new();

        // Set PWD for drive 'C'
        assert!(drive_map.set_pwd(Path::new(r"C:\Users")).is_ok());
        // Or for drive 'c'
        assert!(drive_map.set_pwd(Path::new(r"c:\Users\Example")).is_ok());
        assert_eq!(drive_map.get_pwd('C'), Ok(r"C:\Users\Example".to_string()));
        // or 'c'
        assert_eq!(drive_map.get_pwd('c'), Ok(r"C:\Users\Example".to_string()));

        // Set PWD for drive D
        assert!(drive_map.set_pwd(Path::new(r"D:\Projects")).is_ok());
        assert_eq!(drive_map.get_pwd('D'), Ok(r"D:\Projects".to_string()));

        // Get PWD for drive E (not set yet, but system might happened to
        // have PWD on this drive)
        if let Some(pwd_on_e) = get_full_path_name_w("E:") {
            assert_eq!(drive_map.get_pwd('E'), Ok(pwd_on_e));
        } else {
            assert_eq!(drive_map.get_pwd('E'), Ok(r"E:\".to_string()));
        }
    }

    #[test]
    fn test_set_pwd_invalid_path() {
        let mut drive_map = DriveToPwdMap::new();

        // Invalid path (no drive letter)
        let result = drive_map.set_pwd(Path::new(r"\InvalidPath"));
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), PathError::InvalidPath);
    }

    #[test]
    fn test_get_pwd_invalid_drive() {
        let mut drive_map = DriveToPwdMap::new();

        // Get PWD for a drive not set (e.g., Z)
        assert_eq!(drive_map.get_pwd('Z'), Ok(r"Z:\".to_string()));

        // Invalid drive letter (non-alphabetic)
        assert_eq!(drive_map.get_pwd('1'), Err(PathError::InvalidDriveLetter));
    }
}
