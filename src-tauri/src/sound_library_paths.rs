use super::*;
use std::os::windows::{
    ffi::{OsStrExt, OsStringExt},
    fs::MetadataExt,
};
use windows_sys::Win32::UI::Controls::Dialogs::*;
pub(super) fn wide(p: &Path) -> Vec<u16> {
    p.as_os_str().encode_wide().chain(Some(0)).collect()
}
fn local_drive(kind: u32) -> Result<()> {
    // GetDriveTypeW: removable=2, fixed=3, CD-ROM=5, RAM disk=6.
    if matches!(kind, 2 | 3 | 5 | 6) {
        Ok(())
    } else {
        Err(err("LocalDriveRequired"))
    }
}
pub(super) fn lexical(p: &Path) -> Result<()> {
    use std::path::{Component, Prefix};
    if !p.is_absolute()
        || !matches!(p.components().next(),Some(Component::Prefix(x)) if matches!(x.kind(),Prefix::Disk(_)))
    {
        return Err(err("LocalAbsolutePathRequired"));
    }
    let text = p.to_str().ok_or_else(|| err("NonUnicodePath"))?;
    local_drive(unsafe { GetDriveTypeW(wide(Path::new(&format!("{}\\", &text[..2]))).as_ptr()) })?;
    if text.contains('\0') || text.chars().any(char::is_control) {
        return Err(err("InvalidPath"));
    }
    for part in text[2..].split(['\\', '/']).filter(|s| !s.is_empty()) {
        if part == "." || part == ".." || part.ends_with(['.', ' ']) || part.contains(':') {
            return Err(err("AmbiguousPath"));
        }
        let base = part.split('.').next().unwrap().to_ascii_uppercase();
        if ["CON", "PRN", "AUX", "NUL"].contains(&base.as_str())
            || ((base.starts_with("COM") || base.starts_with("LPT"))
                && base.len() == 4
                && matches!(base.as_bytes()[3], b'1'..=b'9'))
        {
            return Err(err("DevicePathRejected"));
        }
    }
    Ok(())
}
pub(crate) fn safe(p: &Path) -> Result<()> {
    lexical(p)?;
    for a in p.ancestors() {
        match fs::symlink_metadata(a) {
            Ok(m) if m.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 => {
                return Err(err("ReparsePointRejected"))
            }
            Ok(_) => (),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => (),
            Err(e) => return Err(e.into()),
        }
    }
    Ok(())
}
pub(super) fn regular(f: &File) -> Result<()> {
    let m = f.metadata()?;
    let mut info = unsafe { std::mem::zeroed::<BY_HANDLE_FILE_INFORMATION>() };
    if !m.is_file() || m.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
        return Err(err("NotRegularFile"));
    }
    if unsafe { GetFileInformationByHandle(f.as_raw_handle() as _, &mut info) } == 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    if info.nNumberOfLinks != 1 {
        return Err(err("HardLinkRejected"));
    }
    Ok(())
}
fn final_path(f: &File) -> Result<PathBuf> {
    let mut b = vec![0u16; 32768];
    let n = unsafe {
        GetFinalPathNameByHandleW(f.as_raw_handle() as _, b.as_mut_ptr(), b.len() as u32, 0)
    };
    if n == 0 || n as usize >= b.len() {
        return Err(err("FinalPathUnavailable"));
    }
    let s = std::ffi::OsString::from_wide(&b[..n as usize])
        .to_string_lossy()
        .into_owned();
    let path = s
        .strip_prefix("\\\\?\\")
        .ok_or_else(|| err("NonLocalFinalPath"))?;
    let p = PathBuf::from(path);
    lexical(&p)?;
    Ok(p)
}
pub(crate) fn open_read(p: &Path) -> Result<File> {
    safe(p)?;
    let f = OpenOptions::new()
        .read(true)
        .share_mode(FILE_SHARE_READ)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
        .open(p)?;
    regular(&f)?;
    let actual = final_path(&f)?;
    if actual.to_string_lossy().replace('/', "\\").to_lowercase()
        != p.to_string_lossy().replace('/', "\\").to_lowercase()
    {
        return Err(err("FinalPathMismatch"));
    }
    Ok(f)
}
pub(crate) fn read(p: &Path, max: u64) -> Result<Option<Vec<u8>>> {
    let mut f = match open_read(p) {
        Ok(f) => f,
        Err(e) if e.code.starts_with("Io:NotFound:") => return Ok(None),
        Err(e) => return Err(e),
    };
    if f.metadata()?.len() > max {
        return Err(err("ReadLimit"));
    }
    let mut b = vec![];
    (&mut f).take(max + 1).read_to_end(&mut b)?;
    if b.len() as u64 > max {
        return Err(err("ReadLimit"));
    }
    Ok(Some(b))
}
pub(super) fn directory(p: &Path) -> Result<File> {
    safe(p)?;
    let f = OpenOptions::new()
        .read(true)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
        .open(p)?;
    let m = f.metadata()?;
    if !m.is_dir() || m.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
        return Err(err("UnsafeDirectory"));
    }
    Ok(f)
}
pub(crate) fn directory_chain(p: &Path) -> Result<Vec<File>> {
    lexical(p)?;
    let mut paths: Vec<_> = p.ancestors().collect();
    paths.reverse();
    let mut handles = vec![];
    for p in paths {
        safe(p)?;
        if !p.exists() {
            fs::create_dir(p)?;
        }
        handles.push(directory(p)?);
    }
    Ok(handles)
}
pub(crate) fn write_new(p: &Path, b: &[u8]) -> Result<()> {
    safe(p)?;
    let mut f = OpenOptions::new()
        .write(true)
        .create_new(true)
        .share_mode(0)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
        .open(p)?;
    regular(&f)?;
    f.write_all(b)?;
    f.sync_all()?;
    drop(f);
    if read(p, b.len() as u64)?.as_deref() != Some(b) {
        return Err(err("WriteVerifyFailed"));
    }
    Ok(())
}
/// No public from_path constructor. Native picker is the only production issuer.
pub struct SelectedMp3 {
    pub(super) file: File,
    pub(super) path: PathBuf,
    _parents: Vec<File>,
}
impl SelectedMp3 {
    pub fn registration_name(&self) -> Result<String> {
        registration_name(&self.path)
    }
    #[cfg(feature = "custom-playback-test-harness")]
    pub(super) fn verification(bytes: &[u8]) -> Result<Self> {
        let root = crate::settings_disk::verification_root().join("selected-fixtures");
        let _handles = directory_chain(&root)?;
        let path = root.join(format!("{}.mp3", Uuid::new_v4()));
        write_new(&path, bytes)?;
        Self::open(path)
    }
    pub fn pick() -> Result<Option<Self>> {
        let mut buffer = vec![0u16; 32768];
        let filter: Vec<u16> = "MP3 audio (*.mp3)\0*.mp3\0\0".encode_utf16().collect();
        let mut options = unsafe { std::mem::zeroed::<OPENFILENAMEW>() };
        options.lStructSize = std::mem::size_of::<OPENFILENAMEW>() as u32;
        options.lpstrFilter = filter.as_ptr();
        options.lpstrFile = buffer.as_mut_ptr();
        options.nMaxFile = buffer.len() as u32;
        options.Flags = OFN_FILEMUSTEXIST
            | OFN_PATHMUSTEXIST
            | OFN_NOCHANGEDIR
            | OFN_EXPLORER
            | OFN_NODEREFERENCELINKS
            | OFN_DONTADDTORECENT;
        if unsafe { GetOpenFileNameW(&mut options) } == 0 {
            let e = unsafe { CommDlgExtendedError() };
            if e == 0 {
                return Ok(None);
            }
            return Err(err(&format!("PickerError:{e}")));
        }
        let len = buffer
            .iter()
            .position(|c| *c == 0)
            .ok_or_else(|| err("PickerPathTooLong"))?;
        Self::open(PathBuf::from(std::ffi::OsString::from_wide(&buffer[..len]))).map(Some)
    }
    fn open(path: PathBuf) -> Result<Self> {
        lexical(&path)?;
        if !path
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("mp3"))
        {
            return Err(err("Mp3ExtensionRequired"));
        }
        let parents =
            directory_chain_existing(path.parent().ok_or_else(|| err("MissingSourceParent"))?)?;
        let file = open_read(&path)?;
        if file.metadata()?.len() == 0 || file.metadata()?.len() > MAX_FILE {
            return Err(err("FileSizeLimit"));
        }
        Ok(Self {
            file,
            path,
            _parents: parents,
        })
    }
    #[cfg(test)]
    pub(super) fn test(path: PathBuf) -> Result<Self> {
        Self::open(path)
    }
}
fn registration_name(path: &Path) -> Result<String> {
    let filename = path.file_stem().and_then(|s| s.to_str())
        .ok_or_else(|| err("InvalidSoundName"))?.trim();
    if filename.encode_utf16().count() <= 80 {
        super::validate_ui_name(filename)?;
        return Ok(filename.into());
    }
    // Keep the existing catalog name limit without splitting a surrogate pair.
    let mut name = String::new();
    for c in filename.chars() {
        if name.encode_utf16().count() + c.len_utf16() > 80 { break; }
        name.push(c);
    }
    super::validate_ui_name(&name)?;
    Ok(name)
}
#[cfg(test)]
mod naming_tests {
    use super::*;
    #[test]
    fn filename_limit_keeps_unicode_without_extension() {
        let name=registration_name(Path::new(&format!("C:\\{}音源.mp3", "🔔".repeat(60)))).unwrap();
        assert!(name.encode_utf16().count()<=80);
        assert!(!name.ends_with(".mp3"));
        assert_eq!(registration_name(Path::new("C:\\a.MP3")).unwrap(),"a");
        assert_eq!(registration_name(Path::new("C:\\教会.ベル.mp3")).unwrap(),"教会.ベル");
    }
}
#[cfg(test)]
pub(super) fn test_drive(kind: u32) -> Result<()> {
    local_drive(kind)
}
fn directory_chain_existing(p: &Path) -> Result<Vec<File>> {
    lexical(p)?;
    let mut chain: Vec<_> = p.ancestors().collect();
    chain.reverse();
    chain.into_iter().map(directory).collect()
}
