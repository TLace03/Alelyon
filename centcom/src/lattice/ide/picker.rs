//! Windows' own folder picker, for Open Folder…. A folder the person chooses there is attached for the chat with no
//! further dialog (the core's `attach_native`: "the reader chose it in Rust-owned UI"), as a folder dropped on the
//! window is; a path typed into a box still goes through the core's own confirmation.

use std::path::PathBuf;

use windows::Win32::Foundation::ERROR_CANCELLED;
use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE, CoCreateInstance, CoInitializeEx,
    CoTaskMemFree, CoUninitialize,
};
use windows::Win32::UI::Shell::{
    FOS_ALLOWMULTISELECT, FOS_FILEMUSTEXIST, FOS_FORCEFILESYSTEM, FOS_PATHMUSTEXIST, FOS_PICKFOLDERS, FileOpenDialog,
    IFileOpenDialog, SIGDN_FILESYSPATH,
};
use windows::core::{PCWSTR, w};

/// Show the picker (it blocks: call it on a thread of its own); `None` when the person cancels.
pub fn pick_folder() -> Result<Option<PathBuf>, String> {
    // SAFETY: COM is initialised on this thread for the dialog's life and uninitialised after it; the one string the
    // dialog hands back is freed once, after it is copied.
    unsafe {
        let init = CoInitializeEx(None, COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE);
        if init.is_err() {
            return Err(format!("Windows' folder picker could not start ({init:?})."));
        }
        let picked = (|| -> Result<Option<PathBuf>, String> {
            let dialog: IFileOpenDialog = CoCreateInstance(&FileOpenDialog, None, CLSCTX_INPROC_SERVER)
                .map_err(|e| format!("Windows' folder picker could not open ({e})."))?;
            let options = dialog.GetOptions().map_err(|e| e.to_string())?;
            dialog.SetOptions(options | FOS_PICKFOLDERS | FOS_FORCEFILESYSTEM | FOS_PATHMUSTEXIST).map_err(|e| e.to_string())?;
            let _ = dialog.SetTitle(w!("Open a folder in Lattice"));
            let _ = dialog.SetOkButtonLabel(w!("Open folder"));
            if let Err(e) = dialog.Show(None) {
                if e.code() == ERROR_CANCELLED.to_hresult() {
                    return Ok(None);
                }
                return Err(format!("Windows' folder picker failed ({e})."));
            }
            let item = dialog.GetResult().map_err(|e| e.to_string())?;
            let name = item.GetDisplayName(SIGDN_FILESYSPATH).map_err(|e| e.to_string())?;
            let path = name.to_string();
            CoTaskMemFree(Some(name.0 as *const core::ffi::c_void));
            path.map(|p| Some(PathBuf::from(p))).map_err(|_| "The chosen folder's name could not be read.".to_string())
        })();
        CoUninitialize();
        picked
    }
}

/// Windows' file picker for images to attach to a message (several at once); empty when the person cancels. The files
/// are read and checked after (`super::attach`): the picker itself lists every file.
pub fn pick_images() -> Result<Vec<PathBuf>, String> {
    pick_files(w!("Attach images (PNG or JPEG)"), w!("Attach"), true)
}

/// Windows' file picker for photos on Pages: several to attach to a post (`many`), else one for a page's or an
/// organization's photo or banner; empty when the person cancels. The files are read and checked after
/// (`crate::pages`).
pub fn pick_photos(many: bool) -> Result<Vec<PathBuf>, String> {
    if many {
        pick_files(w!("Add photos (JPEG, PNG or WebP)"), w!("Add"), true)
    } else {
        pick_files(w!("Choose a photo (JPEG, PNG or WebP)"), w!("Choose"), false)
    }
}

fn pick_files(title: PCWSTR, ok: PCWSTR, many: bool) -> Result<Vec<PathBuf>, String> {
    // SAFETY: as `pick_folder`: COM for the dialog's life; each string the dialog hands back is freed once, after it
    // is copied. `title` and `ok` are `w!` literals, alive for the program's life.
    unsafe {
        let init = CoInitializeEx(None, COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE);
        if init.is_err() {
            return Err(format!("Windows' file picker could not start ({init:?})."));
        }
        let picked = (|| -> Result<Vec<PathBuf>, String> {
            let dialog: IFileOpenDialog = CoCreateInstance(&FileOpenDialog, None, CLSCTX_INPROC_SERVER)
                .map_err(|e| format!("Windows' file picker could not open ({e})."))?;
            let options = dialog.GetOptions().map_err(|e| e.to_string())?;
            let multiple = if many { FOS_ALLOWMULTISELECT } else { Default::default() };
            dialog.SetOptions(options | multiple | FOS_FORCEFILESYSTEM | FOS_FILEMUSTEXIST).map_err(|e| e.to_string())?;
            let _ = dialog.SetTitle(title);
            let _ = dialog.SetOkButtonLabel(ok);
            if let Err(e) = dialog.Show(None) {
                if e.code() == ERROR_CANCELLED.to_hresult() {
                    return Ok(Vec::new());
                }
                return Err(format!("Windows' file picker failed ({e})."));
            }
            let items = dialog.GetResults().map_err(|e| e.to_string())?;
            let count = items.GetCount().map_err(|e| e.to_string())?;
            let mut paths = Vec::new();
            for i in 0..count {
                let item = items.GetItemAt(i).map_err(|e| e.to_string())?;
                let name = item.GetDisplayName(SIGDN_FILESYSPATH).map_err(|e| e.to_string())?;
                let path = name.to_string();
                CoTaskMemFree(Some(name.0 as *const core::ffi::c_void));
                paths.push(PathBuf::from(path.map_err(|_| "A chosen file's name could not be read.".to_string())?));
            }
            Ok(paths)
        })();
        CoUninitialize();
        picked
    }
}
