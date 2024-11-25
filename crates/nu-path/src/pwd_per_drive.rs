use std::collections::HashMap;
/// Usage for pwd_per_drive on windows
///
/// Upon change PWD, call set_pwd() with absolute path
///
/// Call expand_pwd() with relative path to get absolution path
///
/// ```
/// use std::path::{Path, PathBuf};
/// use nu_path::{expand_pwd, get_env_vars, set_pwd};
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
///
/// // Get env vars for child process
/// use std::collections::HaspMap;
/// let mut env = HashMap::<String, String>::new();
/// get_env_vars(&mut env);
/// assert_eq!(env.get("=C:").unwrap(), r"C:\Users\Home");
/// if let Some(expanded) = expand_pwd(Path::new("D:") {
///     let abs_path = expanded.unwrap().as_path().to_str().expect("OK").to_string();
///     if abs_path.len() > 3 {
///         assert_eq!(env.get("=D:").unwrap(), abs_path);
///     }
/// }
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
        if let Ok(mut pwd_per_drive) = get_shared_drive_pwd_map().lock() {
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
            if let Ok(mut pwd_per_drive) = get_shared_drive_pwd_map().lock() {
                return pwd_per_drive.expand_path(path);
            }
        }
        None
    }

    /// Collect PWD-per-drive as env vars (for child process)
    pub fn get_env_vars(env: &mut HashMap<String, String>) {
        if let Ok(pwd_per_drive) = get_shared_drive_pwd_map().lock() {
            pwd_per_drive.get_env_vars(env);
        }
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
            let env_var = format!("={}:", drive_letter);
            if let Ok(env_pwd) = std::env::var(&env_var) {
                if env_pwd.len() > 3 {
                    map[drive_index] = Some(env_pwd);
                    std::env::remove_var(env_var);
                    continue;
                }
            }
            if let Some(pwd) = get_full_path_name_w(&format!("{}:", drive_letter)) {
                if pwd.len() > 3 {
                    map[drive_index] = Some(pwd);
                }
            }
        }
        Self { map }
    }

    /// Collect PWD-per-drive as env vars (for child process)
    pub fn get_env_vars(&self, env: &mut HashMap<String, String>) {
        for (drive_index, drive_letter) in ('A'..='Z').enumerate() {
            if let Some(pwd) = self.map[drive_index].clone() {
                if pwd.len() > 3 {
                    let env_var_for_drive = format!("={}:", drive_letter);
                    env.insert(env_var_for_drive, pwd);
                }
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
    /// Remember effective result from GetFullPathNameW
    fn get_pwd(&mut self, drive_letter: char) -> Result<String, PathError> {
        if drive_letter.is_ascii_alphabetic() {
            let drive_letter = drive_letter.to_ascii_uppercase();
            let drive_index = drive_letter as usize - 'A' as usize;
            Ok(self.map[drive_index].clone().unwrap_or_else(|| {
                if let Some(sys_pwd) = get_full_path_name_w(&format!("{}:", drive_letter)) {
                    if sys_pwd.len() > 3 {
                        self.map[drive_index] = Some(sys_pwd.clone());
                    }
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
fn get_shared_drive_pwd_map() -> &'static Mutex<DriveToPwdMap> {
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
        // Set PWD for drive E
        assert!(set_pwd(Path::new(r"E:\Users\Home")).is_ok());

        // Expand a relative path
        let expanded = expand_pwd(Path::new("e:test"));
        assert_eq!(expanded, Some(PathBuf::from(r"E:\Users\Home\test")));

        // Will NOT expand an absolute path
        let expanded = expand_pwd(Path::new(r"E:\absolute\path"));
        assert_eq!(expanded, None);

        // Expand with no drive letter
        let expanded = expand_pwd(Path::new(r"\no_drive"));
        assert_eq!(expanded, None);

        // Expand with no PWD set for the drive
        let expanded = expand_pwd(Path::new("F:test"));
        if let Some(sys_abs) = get_full_path_name_w("F:") {
            assert_eq!(
                expanded,
                Some(PathBuf::from(format!(
                    "{}test",
                    DriveToPwdMap::ensure_trailing_delimiter(&sys_abs)
                )))
            );
        } else {
            assert_eq!(expanded, Some(PathBuf::from(r"F:\test")));
        }
    }

    #[test]
    fn test_read_pwd_per_drive_at_start_up() {
        std::env::set_var("=G:", r"G:\Users\Nushell");
        std::env::set_var("=H:", r"h:\Share\Nushell");
        let mut map = DriveToPwdMap::new();
        assert_eq!(
           map.expand_path(Path::new("g:")),
           Some(PathBuf::from(r"G:\Users\Nushell\"))
        );
        assert_eq!(
            map.expand_path(Path::new("H:")),
            Some(PathBuf::from(r"H:\Share\Nushell\"))
        );

        std::env::remove_var("=G:");
        std::env::remove_var("=H:");
    }

    #[test]
    fn test_get_env_vars() {
        let mut map = DriveToPwdMap::new();
        map.set_pwd(&Path::new(r"I:\Home")).unwrap();
        map.set_pwd(&Path::new(r"j:\User")).unwrap();

        let mut env = HashMap::<String, String>::new();
        map.get_env_vars(&mut env);
        assert_eq!(env.get("=I:").unwrap(), r"I:\Home");
        assert_eq!(env.get("=J:").unwrap(), r"J:\User");
    }

    #[test]
    fn test_shared_set_and_get_pwd() {
        // To avoid conflict with other test threads (on testing result),
        // use different drive set in multiple shared_map tests
        let drive_pwd_map = get_shared_drive_pwd_map();
        {
            let mut map = drive_pwd_map.lock().unwrap();

            // Set PWD for drive K
            assert!(map.set_pwd(Path::new(r"k:\Users\Example")).is_ok());
        }

        {
            let mut map = drive_pwd_map.lock().unwrap();

            // Get PWD for drive K
            assert_eq!(map.get_pwd('K'), Ok(r"K:\Users\Example".to_string()));

            // Get PWD for drive E (not set, should return E:\) ???
            // 11-21-2024 happened to start nushell from drive E:,
            // run toolkit test 'toolkit check pr' then this test failed
            // since for drive that has not bind PWD, if the drive really exists
            // in system and current directory is not drive root, this test will
            // fail if assuming result should be r"X:\", and there might also have
            // other cases tested by other threads which might change PWD.
            if let Some(pwd_on_e) = get_full_path_name_w("L:") {
                assert_eq!(map.get_pwd('L'), Ok(pwd_on_e));
            } else {
                assert_eq!(map.get_pwd('l'), Ok(r"L:\".to_string()));
            }
        }
    }

    #[test]
    fn test_expand_path() {
        let mut drive_map = DriveToPwdMap::new();

        // Set PWD for drive 'M:'
        assert_eq!(drive_map.set_pwd(Path::new(r"M:\Users")), Ok(()));
        // or 'm:'
        assert_eq!(drive_map.set_pwd(Path::new(r"m:\Users\Home")), Ok(()));

        // Expand a relative path on "M:"
        let expanded = drive_map.expand_path(Path::new(r"M:test"));
        assert_eq!(expanded, Some(PathBuf::from(r"M:\Users\Home\test")));
        // or on "m:"
        let expanded = drive_map.expand_path(Path::new(r"m:test"));
        assert_eq!(expanded, Some(PathBuf::from(r"M:\Users\Home\test")));

        // Expand an absolute path
        let expanded = drive_map.expand_path(Path::new(r"m:\absolute\path"));
        assert_eq!(expanded, None);

        // Expand with no drive letter
        let expanded = drive_map.expand_path(Path::new(r"\no_drive"));
        assert_eq!(expanded, None);

        // Expand with no PWD set for the drive
        let expanded = drive_map.expand_path(Path::new("N:test"));
        if let Some(pwd_on_drive) = get_full_path_name_w("N:") {
            assert_eq!(
                expanded,
                Some(PathBuf::from(format!(
                    r"{}test",
                    DriveToPwdMap::ensure_trailing_delimiter(&pwd_on_drive)
                )))
            );
        } else {
            assert_eq!(expanded, Some(PathBuf::from(r"N:\test")));
        }
    }

    #[test]
    fn test_set_and_get_pwd() {
        let mut drive_map = DriveToPwdMap::new();

        // Set PWD for drive 'O'
        assert!(drive_map.set_pwd(Path::new(r"O:\Users")).is_ok());
        // Or for drive 'o'
        assert!(drive_map.set_pwd(Path::new(r"o:\Users\Example")).is_ok());
        // Get PWD for drive 'O'
        assert_eq!(drive_map.get_pwd('O'), Ok(r"O:\Users\Example".to_string()));
        // or 'o'
        assert_eq!(drive_map.get_pwd('o'), Ok(r"O:\Users\Example".to_string()));

        // Get PWD for drive P (not set yet, but system might already
        // have PWD on this drive)
        if let Some(pwd_on_drive) = get_full_path_name_w("P:") {
            assert_eq!(drive_map.get_pwd('P'), Ok(pwd_on_drive));
        } else {
            assert_eq!(drive_map.get_pwd('P'), Ok(r"P:\".to_string()));
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
