//! A menu that floats over the page under what opened it, as the social sites open theirs: it takes no room in the
//! column and pushes nothing down. While it is open the page under it is still, as under X's menus: a press outside
//! it, or Escape, closes it (`on_close`) without acting on what lies under it.

use iced::advanced::layout::{self, Layout};
use iced::advanced::overlay;
use iced::advanced::renderer;
use iced::advanced::widget::{self, Tree, Widget};
use iced::advanced::{Clipboard, Shell};
use iced::{Element, Event, Length, Point, Rectangle, Renderer, Size, Theme, Vector, keyboard, mouse};

/// How a menu lines up with what opened it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    /// Its right edge under the anchor's, as a ⋯ menu opens.
    Right,
    /// Its middle under the anchor's, as a row of reactions opens.
    Centre,
    /// Its left edge under the anchor's, as the people a mention may name open under the text.
    Left,
}

/// `anchor`, with `menu` floating under it while there is one; its right edge under the anchor's.
pub fn popover<'a, Message: Clone + 'a>(
    anchor: impl Into<Element<'a, Message>>,
    menu: Option<Element<'a, Message>>,
    on_close: Message,
) -> Element<'a, Message> {
    popover_at(anchor, menu, on_close, Side::Right)
}

/// `anchor`, with `menu` floating under it, lined up on `side`.
pub fn popover_at<'a, Message: Clone + 'a>(
    anchor: impl Into<Element<'a, Message>>,
    menu: Option<Element<'a, Message>>,
    on_close: Message,
    side: Side,
) -> Element<'a, Message> {
    let open = menu.is_some();
    Element::new(Popover { anchor: anchor.into(), menu: menu.unwrap_or_else(|| iced::widget::space().into()), open, on_close, side })
}

struct Popover<'a, Message> {
    anchor: Element<'a, Message>,
    menu: Element<'a, Message>,
    open: bool,
    on_close: Message,
    side: Side,
}

impl<Message: Clone> Widget<Message, Theme, Renderer> for Popover<'_, Message> {
    fn children(&self) -> Vec<Tree> {
        vec![Tree::new(&self.anchor), Tree::new(&self.menu)]
    }

    fn diff(&self, tree: &mut Tree) {
        tree.diff_children(&[&self.anchor, &self.menu]);
    }

    fn size(&self) -> Size<Length> {
        self.anchor.as_widget().size()
    }

    fn size_hint(&self) -> Size<Length> {
        self.anchor.as_widget().size_hint()
    }

    fn layout(&mut self, tree: &mut Tree, renderer: &Renderer, limits: &layout::Limits) -> layout::Node {
        self.anchor.as_widget_mut().layout(&mut tree.children[0], renderer, limits)
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        viewport: &Rectangle,
    ) {
        self.anchor.as_widget_mut().update(&mut tree.children[0], event, layout, cursor, renderer, clipboard, shell, viewport);
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut Renderer,
        theme: &Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        self.anchor.as_widget().draw(&tree.children[0], renderer, theme, style, layout, cursor, viewport);
    }

    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &Renderer,
    ) -> mouse::Interaction {
        self.anchor.as_widget().mouse_interaction(&tree.children[0], layout, cursor, viewport, renderer)
    }

    fn operate(&mut self, tree: &mut Tree, layout: Layout<'_>, renderer: &Renderer, operation: &mut dyn widget::Operation) {
        self.anchor.as_widget_mut().operate(&mut tree.children[0], layout, renderer, operation);
    }

    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut Tree,
        layout: Layout<'b>,
        renderer: &Renderer,
        viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'b, Message, Theme, Renderer>> {
        let (anchor_tree, menu_tree) = tree.children.split_at_mut(1);
        let inner = self.anchor.as_widget_mut().overlay(&mut anchor_tree[0], layout, renderer, viewport, translation);
        if !self.open {
            return inner;
        }
        let floating = overlay::Element::new(Box::new(Floating {
            anchor: layout.bounds() + translation,
            menu: &mut self.menu,
            tree: &mut menu_tree[0],
            on_close: self.on_close.clone(),
            side: self.side,
        }));
        Some(match inner {
            Some(inner) => overlay::Group::with_children(vec![inner, floating]).overlay(),
            None => floating,
        })
    }
}

/// The open menu, laid over the page.
struct Floating<'a, 'b, Message> {
    anchor: Rectangle,
    menu: &'b mut Element<'a, Message>,
    tree: &'b mut Tree,
    on_close: Message,
    side: Side,
}

/// The room between what opened a menu and the menu.
const GAP: f32 = 4.0;

/// Where a menu `size` goes for an anchor in a window `bounds`: lined up on `side`, under the anchor when it fits
/// below and over it when it does not, and inside the window.
pub fn place(anchor: Rectangle, size: Size, bounds: Size, side: Side) -> Point {
    let below = anchor.y + anchor.height + GAP;
    let above = anchor.y - GAP - size.height;
    let y = if below + size.height <= bounds.height || above < 0.0 { below } else { above };
    let x = match side {
        Side::Right => anchor.x + anchor.width - size.width,
        Side::Centre => anchor.x + (anchor.width - size.width) / 2.0,
        Side::Left => anchor.x,
    };
    Point::new(x.clamp(0.0, (bounds.width - size.width).max(0.0)), y)
}

impl<Message: Clone> overlay::Overlay<Message, Theme, Renderer> for Floating<'_, '_, Message> {
    fn layout(&mut self, renderer: &Renderer, bounds: Size) -> layout::Node {
        let node = self.menu.as_widget_mut().layout(self.tree, renderer, &layout::Limits::new(Size::ZERO, bounds));
        let at = place(self.anchor, node.size(), bounds, self.side);
        node.move_to(at)
    }

    fn draw(&self, renderer: &mut Renderer, theme: &Theme, style: &renderer::Style, layout: Layout<'_>, cursor: mouse::Cursor) {
        self.menu.as_widget().draw(self.tree, renderer, theme, style, layout, cursor, &layout.bounds());
    }

    fn update(
        &mut self,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
    ) {
        self.menu.as_widget_mut().update(self.tree, event, layout, cursor, renderer, clipboard, shell, &layout.bounds());
        if shell.is_event_captured() {
            return;
        }
        let outside = matches!(event, Event::Mouse(mouse::Event::ButtonPressed(_))) && !cursor.is_over(layout.bounds());
        let escape = matches!(
            event,
            Event::Keyboard(keyboard::Event::KeyPressed { key: keyboard::Key::Named(keyboard::key::Named::Escape), .. })
        );
        if outside || escape {
            shell.publish(self.on_close.clone());
            shell.capture_event();
        }
    }

    fn mouse_interaction(&self, layout: Layout<'_>, cursor: mouse::Cursor, renderer: &Renderer) -> mouse::Interaction {
        // never None while open: the page under the menu keeps still, as under a menu's backdrop
        match self.menu.as_widget().mouse_interaction(self.tree, layout, cursor, &layout.bounds(), renderer) {
            mouse::Interaction::None => mouse::Interaction::Idle,
            other => other,
        }
    }

    fn operate(&mut self, layout: Layout<'_>, renderer: &Renderer, operation: &mut dyn widget::Operation) {
        self.menu.as_widget_mut().operate(self.tree, layout, renderer, operation);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_menu_opens_under_its_anchor_with_their_right_edges_aligned() {
        let anchor = Rectangle { x: 500.0, y: 100.0, width: 30.0, height: 30.0 };
        let at = place(anchor, Size::new(200.0, 150.0), Size::new(1200.0, 900.0), Side::Right);
        assert_eq!(at, Point::new(330.0, 134.0));
        let centred = place(anchor, Size::new(200.0, 150.0), Size::new(1200.0, 900.0), Side::Centre);
        assert_eq!(centred, Point::new(415.0, 134.0), "its middle under the anchor's");
    }

    #[test]
    fn a_menu_without_room_below_opens_above_and_stays_inside_the_window() {
        let low = Rectangle { x: 20.0, y: 800.0, width: 30.0, height: 30.0 };
        let at = place(low, Size::new(200.0, 150.0), Size::new(1200.0, 900.0), Side::Right);
        assert_eq!(at, Point::new(0.0, 646.0), "over the anchor, and not off the window's left edge");
        let top = Rectangle { x: 20.0, y: 10.0, width: 30.0, height: 30.0 };
        assert_eq!(place(top, Size::new(200.0, 950.0), Size::new(1200.0, 900.0), Side::Right).y, 44.0, "no room either way: below");
    }
}
