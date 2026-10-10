//! Windows' own say on motion: Settings > Accessibility > Visual effects > "Animation effects" (the "Show animations
//! in Windows" of the classic panel), read with `SystemParametersInfoW(SPI_GETCLIENTAREAANIMATION)`. With nothing
//! kept in Alelyon's preferences, the backdrop moves only when this is on.

/// Whether Windows shows animations; None where it cannot be asked.
#[cfg(windows)]
pub fn animations_on() -> Option<bool> {
    #[link(name = "user32")]
    unsafe extern "system" {
        fn SystemParametersInfoW(action: u32, param: u32, pv: *mut core::ffi::c_void, win_ini: u32) -> i32;
    }
    const SPI_GETCLIENTAREAANIMATION: u32 = 0x1042;
    let mut on: i32 = 0;
    // SAFETY: for this action the system writes one BOOL (an i32) through `pv`, which points at `on`, a live i32 of
    // ours for the whole call; nothing else is read or kept.
    let ok = unsafe { SystemParametersInfoW(SPI_GETCLIENTAREAANIMATION, 0, (&mut on as *mut i32).cast(), 0) };
    (ok != 0).then_some(on != 0)
}

#[cfg(not(windows))]
pub fn animations_on() -> Option<bool> {
    None
}

#[cfg(test)]
mod tests {
    #[test]
    fn windows_answers_whether_it_animates() {
        // the answer is this PC's setting; that there is one is what is checked
        assert_eq!(super::animations_on().is_some(), cfg!(windows));
    }
}
