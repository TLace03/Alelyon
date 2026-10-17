//! Pages' keys, as X binds its own (rules A1 and A2 of the interface principles): j and k move between the posts of
//! the column, l likes the one chosen, r replies to it, t reposts or quotes it, b saves it, o or Enter opens it, n writes
//! a new post, / goes to search and . reads the column again. They act only while no box has the keyboard, and never
//! with Ctrl or Alt held, so a box's own keys and the window's are never taken.

use iced::advanced::widget::operation::{Operation, Outcome, Scrollable};
use iced::advanced::widget::Id;
use iced::keyboard::{Event, Key, key::Named};
use iced::{Rectangle, Vector, event, window};

use super::Msg;

/// What a key asks of the page.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PageKey {
    Next,
    Previous,
    Like,
    Reply,
    Repost,
    Bookmark,
    Open,
    NewPost,
    Search,
    Refresh,
    /// The viewer's previous or next photo (the arrow keys, while a photo is open).
    PhotoPrevious,
    PhotoNext,
}

/// The keys as the shortcut sheet lists them (`crate::shortcuts::PAGES`).
pub const SHEET: &[(&str, &str)] = &[
    ("j, k", "The next or previous post"),
    ("l", "Like the post"),
    ("r", "Reply to it"),
    ("t", "Repost or quote it"),
    ("b", "Save it to Bookmarks, or take it off"),
    ("o or Enter", "Open its conversation"),
    ("n", "Write a new post"),
    ("/", "Search Pages"),
    (".", "Read the column again"),
    ("Left, Right", "The previous or next photo, while one is open"),
    ("Esc", "Close an open menu, or the photo on show"),
];

/// A key the page acts on, when no box took it and no Ctrl or Alt is held.
pub fn keys(event: iced::Event, status: event::Status, _window: window::Id) -> Option<Msg> {
    let iced::Event::Keyboard(Event::KeyPressed { key, modifiers, .. }) = event else { return None };
    if status == event::Status::Captured || modifiers.command() || modifiers.alt() || modifiers.logo() {
        return None;
    }
    key_for(&key).map(Msg::Key)
}

/// The page's act for a key, whoever pressed it.
pub fn key_for(key: &Key) -> Option<PageKey> {
    Some(match key.as_ref() {
        Key::Character("j") => PageKey::Next,
        Key::Character("k") => PageKey::Previous,
        Key::Character("l") => PageKey::Like,
        Key::Character("r") => PageKey::Reply,
        Key::Character("t") => PageKey::Repost,
        Key::Character("b") => PageKey::Bookmark,
        Key::Character("o") | Key::Named(Named::Enter) => PageKey::Open,
        Key::Character("n") => PageKey::NewPost,
        Key::Character("/") => PageKey::Search,
        Key::Character(".") => PageKey::Refresh,
        Key::Named(Named::ArrowLeft) => PageKey::PhotoPrevious,
        Key::Named(Named::ArrowRight) => PageKey::PhotoNext,
        _ => return None,
    })
}

/// The column's scrollable.
pub const FEED: &str = "pages-feed";
/// The search field, the composer's text, and the reply field of a conversation.
pub const SEARCH: &str = "pages-search";
pub const COMPOSER: &str = "pages-composer";
pub const REPLY: &str = "pages-reply";

/// A post's row in the column, by the post's id.
pub fn row_id(post: &str) -> Id {
    Id::from(format!("pages-post-{post}"))
}

/// How far the column must scroll for `row` to show whole in a view `bounds` of `content` scrolled by `scrolled`:
/// the new offset, or None when it shows already. A row above the view comes to its top, one below to its foot, with
/// a little room either side.
pub fn reveal(bounds: Rectangle, content: Rectangle, scrolled: Vector, row: Rectangle) -> Option<f32> {
    const ROOM: f32 = 8.0;
    let top = row.y - content.y;
    let foot = top + row.height;
    if top < scrolled.y {
        Some((top - ROOM).max(0.0))
    } else if foot > scrolled.y + bounds.height {
        Some((foot - bounds.height + ROOM).min(top))
    } else {
        None
    }
}

/// Finds the column and a row in it, and how far the column must scroll to show the row (`reveal`).
pub struct Reveal {
    pub feed: Id,
    pub row: Id,
    view: Option<(Rectangle, Rectangle, Vector)>,
    found: Option<Rectangle>,
}

impl Reveal {
    pub fn new(row: Id) -> Self {
        Reveal { feed: Id::new(FEED), row, view: None, found: None }
    }
}

impl Operation<Option<f32>> for Reveal {
    fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn Operation<Option<f32>>)) {
        operate(self);
    }

    fn container(&mut self, id: Option<&Id>, bounds: Rectangle) {
        if id == Some(&self.row) {
            self.found = Some(bounds);
        }
    }

    fn scrollable(&mut self, id: Option<&Id>, bounds: Rectangle, content: Rectangle, translation: Vector, _state: &mut dyn Scrollable) {
        if id == Some(&self.feed) {
            self.view = Some((bounds, content, translation));
        }
    }

    fn finish(&self) -> Outcome<Option<f32>> {
        Outcome::Some(match (self.view, self.found) {
            (Some((bounds, content, scrolled)), Some(row)) => reveal(bounds, content, scrolled, row),
            _ => None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use iced::keyboard::Modifiers;

    fn press(key: Key, modifiers: Modifiers) -> iced::Event {
        iced::Event::Keyboard(Event::KeyPressed {
            key: key.clone(),
            modified_key: key,
            physical_key: iced::keyboard::key::Physical::Unidentified(iced::keyboard::key::NativeCode::Unidentified),
            location: iced::keyboard::Location::Standard,
            modifiers,
            text: None,
            repeat: false,
        })
    }

    #[test]
    fn the_keys_act_only_when_no_box_took_them_and_no_ctrl_or_alt_is_held() {
        let j = || press(Key::Character("j".into()), Modifiers::empty());
        assert!(matches!(keys(j(), event::Status::Ignored, window::Id::unique()), Some(Msg::Key(PageKey::Next))));
        assert!(keys(j(), event::Status::Captured, window::Id::unique()).is_none(), "a box took it");
        assert!(keys(press(Key::Character("j".into()), Modifiers::CTRL), event::Status::Ignored, window::Id::unique()).is_none());
        assert!(keys(press(Key::Character("j".into()), Modifiers::ALT), event::Status::Ignored, window::Id::unique()).is_none());
        assert_eq!(key_for(&Key::Named(Named::Enter)), Some(PageKey::Open));
        assert_eq!(key_for(&Key::Character("q".into())), None);
    }

    #[test]
    fn a_row_out_of_view_is_scrolled_to_and_one_in_view_is_left_alone() {
        let view = Rectangle { x: 0.0, y: 53.0, width: 600.0, height: 800.0 };
        let content = Rectangle { x: 0.0, y: 53.0, width: 600.0, height: 5_000.0 };
        let row = |top: f32| Rectangle { x: 0.0, y: 53.0 + top, width: 600.0, height: 200.0 };
        assert_eq!(reveal(view, content, Vector::new(0.0, 0.0), row(100.0)), None, "on show");
        assert_eq!(reveal(view, content, Vector::new(0.0, 0.0), row(700.0)), Some(108.0), "its foot comes to the view's");
        assert_eq!(reveal(view, content, Vector::new(0.0, 1_000.0), row(900.0)), Some(892.0), "its top comes to the view's");
        let tall = Rectangle { height: 1_200.0, ..row(2_000.0) };
        assert_eq!(reveal(view, content, Vector::new(0.0, 0.0), tall), Some(2_000.0), "taller than the view: its top");
    }
}
