//! The mouse as the system knows it, for a drag that must never outlive the press that began it.
//!
//! A widget learns a button was let go only if the release reaches it; iced's stacks stop passing an event on once a
//! layer above has taken it, so a drag could miss its release and go on turning Sinai under a pointer that merely
//! moves (seen in the Appearance creator, 2026-10-07). Asking Windows whether the left button is held
//! (`GetAsyncKeyState`, through the swap a person may have set) settles it on every move.

/// Whether the primary (left) button is held now; None where the system cannot be asked.
#[cfg(windows)]
pub fn left_held() -> Option<bool> {
    #[link(name = "user32")]
    unsafe extern "system" {
        fn GetAsyncKeyState(key: i32) -> i16;
        fn GetSystemMetrics(index: i32) -> i32;
    }
    const VK_LBUTTON: i32 = 0x01;
    const VK_RBUTTON: i32 = 0x02;
    const SM_SWAPBUTTON: i32 = 23;
    // SAFETY: both calls take plain integers and return plain integers; neither touches memory of ours.
    unsafe {
        // the key codes name physical buttons: with the buttons swapped, the primary one is the right
        let key = if GetSystemMetrics(SM_SWAPBUTTON) != 0 { VK_RBUTTON } else { VK_LBUTTON };
        Some(GetAsyncKeyState(key) as u16 & 0x8000 != 0)
    }
}

#[cfg(not(windows))]
pub fn left_held() -> Option<bool> {
    None
}

/// Whether a drag goes on: not when the system says the button is up; where it cannot say, the drag's own release
/// is all there is.
pub fn drag_goes_on(held: Option<bool>) -> bool {
    held != Some(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_drag_ends_when_the_button_is_up_whatever_reached_the_widget() {
        assert!(drag_goes_on(Some(true)));
        assert!(!drag_goes_on(Some(false)), "released where the widget did not hear it");
        assert!(drag_goes_on(None), "no system to ask: the widget's own release decides");
        // nobody is pressing a mouse button while the tests run
        assert_eq!(left_held().map(|_| ()), if cfg!(windows) { Some(()) } else { None });
    }
}
