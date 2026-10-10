//! Windows' own file picker, for Add a GGUF file…: the person chooses the file there; what is done with it (a hard
//! link into the models folder, after its checks) is `models::add_gguf`'s.

use std::path::PathBuf;

use windows::Win32::Foundation::ERROR_CANCELLED;
use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE, CoCreateInstance, CoInitializeEx,
    CoTaskMemFree, CoUninitialize,
};
use windows::Win32::UI::Shell::{
    FOS_FILEMUSTEXIST, FOS_FORCEFILESYSTEM, FOS_PATHMUSTEXIST, FileOpenDialog, IFileOpenDialog, SIGDN_FILESYSPATH,
};
use windows::core::w;

/// Show the picker (it blocks: call it on a thread of its own); `None` when the person cancels.
pub fn pick_file() -> Result<Option<PathBuf>, String> {
    // SAFETY: COM is initialised on this thread for the dialog's life and uninitialised after it; the one string the
    // dialog hands back is freed once, after it is copied.
    unsafe {
        let init = CoInitializeEx(None, COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE);
        if init.is_err() {
            return Err(format!("Windows' file picker could not start ({init:?})."));
        }
        let picked = (|| -> Result<Option<PathBuf>, String> {
            let dialog: IFileOpenDialog = CoCreateInstance(&FileOpenDialog, None, CLSCTX_INPROC_SERVER)
                .map_err(|e| format!("Windows' file picker could not open ({e})."))?;
            let options = dialog.GetOptions().map_err(|e| e.to_string())?;
            dialog
                .SetOptions(options | FOS_FORCEFILESYSTEM | FOS_PATHMUSTEXIST | FOS_FILEMUSTEXIST)
                .map_err(|e| e.to_string())?;
            let _ = dialog.SetTitle(w!("Add a GGUF model"));
            let _ = dialog.SetOkButtonLabel(w!("Add"));
            if let Err(e) = dialog.Show(None) {
                if e.code() == ERROR_CANCELLED.to_hresult() {
                    return Ok(None);
                }
                return Err(format!("Windows' file picker failed ({e})."));
            }
            let item = dialog.GetResult().map_err(|e| e.to_string())?;
            let name = item.GetDisplayName(SIGDN_FILESYSPATH).map_err(|e| e.to_string())?;
            let path = name.to_string();
            CoTaskMemFree(Some(name.0 as *const core::ffi::c_void));
            path.map(|p| Some(PathBuf::from(p))).map_err(|_| "The chosen file's name could not be read.".to_string())
        })();
        CoUninitialize();
        picked
    }
}
