//! Windows notifications for Alelyon: what a session needs, shown by Windows while the person is in another window
//! (`lattice::notify` says what; the app says when).
//!
//! Windows shows a desktop app's notification only for an application identity it knows from a Start-menu
//! shortcut. So the first notification, once the person has switched them on in Settings, makes
//! `%APPDATA%\Microsoft\Windows\Start Menu\Programs\Alelyon (notifications).lnk`, pointing at this program with that
//! identity ([`APP_ID`]), and this process takes the identity. Nothing else is written, and a failure only means no
//! notification: the taskbar still flashes.

/// The application identity Alelyon's notifications are shown under.
pub const APP_ID: &str = "Alelyon.App";
/// The Start-menu shortcut that carries it.
pub const SHORTCUT: &str = "Alelyon (notifications).lnk";

/// `text` safe inside the notification's XML.
pub fn escaped(text: &str) -> String {
    text.chars()
        .take(200)
        .map(|c| match c {
            '&' => "&amp;".to_string(),
            '<' => "&lt;".to_string(),
            '>' => "&gt;".to_string(),
            '"' => "&quot;".to_string(),
            '\'' => "&apos;".to_string(),
            c if c.is_control() => " ".to_string(),
            c => c.to_string(),
        })
        .collect()
}

/// The notification's XML.
pub fn toast_xml(heading: &str, line: &str) -> String {
    format!(
        "<toast><visual><binding template=\"ToastGeneric\"><text>{}</text><text>{}</text></binding></visual></toast>",
        escaped(heading),
        escaped(line)
    )
}

/// Show one notification (blocking: COM and the shortcut; run it off the window's thread).
#[cfg(windows)]
pub fn show(heading: &str, line: &str) -> Result<(), String> {
    use windows::Data::Xml::Dom::XmlDocument;
    use windows::UI::Notifications::{ToastNotification, ToastNotificationManager};
    use windows::core::HSTRING;

    ensure_identity()?;
    let doc = XmlDocument::new().map_err(|e| e.message())?;
    doc.LoadXml(&HSTRING::from(toast_xml(heading, line))).map_err(|e| e.message())?;
    let toast = ToastNotification::CreateToastNotification(&doc).map_err(|e| e.message())?;
    let notifier = ToastNotificationManager::CreateToastNotifierWithId(&HSTRING::from(APP_ID)).map_err(|e| e.message())?;
    notifier.Show(&toast).map_err(|e| e.message())
}

#[cfg(not(windows))]
pub fn show(_heading: &str, _line: &str) -> Result<(), String> {
    Err("notifications are shown on Windows only".into())
}

/// This process takes [`APP_ID`], and the Start-menu shortcut that carries it is made when it is missing.
#[cfg(windows)]
fn ensure_identity() -> Result<(), String> {
    use windows::Win32::Foundation::PROPERTYKEY;
    use windows::Win32::System::Com::StructuredStorage::{PROPVARIANT, PropVariantClear};
    use windows::Win32::System::Com::{
        CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx, IPersistFile,
    };
    use windows::Win32::UI::Shell::PropertiesSystem::IPropertyStore;
    use windows::Win32::UI::Shell::{IShellLinkW, SetCurrentProcessExplicitAppUserModelID, ShellLink};
    use windows::core::{GUID, HSTRING, Interface};

    // System.AppUserModel.ID
    const APP_USER_MODEL_ID: PROPERTYKEY =
        PROPERTYKEY { fmtid: GUID::from_u128(0x9f4c2855_9f79_4b39_a8d0_e1d42de1d5f3), pid: 5 };

    let id = HSTRING::from(APP_ID);
    // SAFETY: a plain call with a valid wide string.
    unsafe { SetCurrentProcessExplicitAppUserModelID(&id) }.map_err(|e| e.message())?;
    let programs = std::env::var_os("APPDATA")
        .map(|d| std::path::PathBuf::from(d).join(r"Microsoft\Windows\Start Menu\Programs"))
        .ok_or("APPDATA is not set")?;
    let link = programs.join(SHORTCUT);
    if link.exists() {
        return Ok(());
    }
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    // SAFETY: COM on this thread (a second initialisation answers S_FALSE, which is fine); the shell link and its
    // property store are used only here, and the PROPVARIANT is cleared after use.
    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        let shell: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER).map_err(|e| e.message())?;
        shell.SetPath(&HSTRING::from(exe.as_os_str())).map_err(|e| e.message())?;
        let store: IPropertyStore = shell.cast().map_err(|e| e.message())?;
        let mut value = PROPVARIANT::from(APP_ID);
        let set = store.SetValue(&APP_USER_MODEL_ID, &value).and_then(|_| store.Commit());
        let _ = PropVariantClear(&mut value);
        set.map_err(|e| e.message())?;
        let file: IPersistFile = shell.cast().map_err(|e| e.message())?;
        file.Save(&HSTRING::from(link.as_os_str()), true).map_err(|e| e.message())?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_notifications_words_are_escaped_and_bounded() {
        assert_eq!(
            toast_xml("\u{201c}A <b> & \"c\"\u{201d} needs you", "line\none"),
            "<toast><visual><binding template=\"ToastGeneric\"><text>\u{201c}A &lt;b&gt; &amp; &quot;c&quot;\u{201d} needs you</text><text>line one</text></binding></visual></toast>"
        );
        assert_eq!(escaped(&"x".repeat(500)).chars().count(), 200);
    }
}
