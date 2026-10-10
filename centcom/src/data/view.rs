//! What the Data page draws: the databases on the left, a database's tables, and a table's rows.

use iced::widget::scrollable::{Direction, Scrollbar};
use iced::widget::{Column, Row, button, column, container, row, scrollable, space, text_input};
use iced::{Alignment, Element, Length};

use crate::theme;

use super::stores::{Cell, STORES};
use super::{Msg, PAGE, State};
use crate::ui::{self, card, label, mono, note, strong};

type El<'a> = Element<'a, Msg>;

const PURPOSE: &str = "The project's databases, read-only, as tables to browse: the fleet's bus, the PR relay and its receipts, \
                       the ledgers, Lattice's workspaces, and Sinai's memory and experience.";

/// A cell's width in the grid, and how much of its text the grid shows.
const CELL_W: f32 = 190.0;
const CELL_CHARS: usize = 60;

pub fn view(state: &State, phase: f32) -> El<'_> {
    let mut page = Column::new().spacing(18).push(ui::heading("Data", PURPOSE));
    if let Err(why) = &state.places {
        return page.push(ui::notice(why.as_str(), theme::CAUTION)).into();
    }
    page = page.push(note(
        "Nothing here writes: each database is read as it lies, or beside the program that has it open, and a column whose \
         name says it holds a secret is never read. Markets data and the account stores are not listed.",
    ));
    page.push(row![shelf(state), container(space().width(1)).height(Length::Fill).style(theme::line), main(state, phase)].spacing(18))
        .into()
}

/// The databases, each with its size and how it would be read.
fn shelf(state: &State) -> El<'_> {
    let mut list = Column::new().spacing(6).width(290);
    for (i, store) in STORES.iter().enumerate() {
        let chosen = state.store == Some(i);
        let shelf = state.shelf.get(i);
        let sub = match shelf {
            None => "…".to_string(),
            Some(s) => match (&s.plan, s.size) {
                (Ok(access), Some(size)) => format!("{}, {}", ui::bytes(size), access.words()),
                (Err(why), _) => why.clone(),
                (Ok(access), None) => access.words().to_string(),
            },
        };
        let mut words =
            column![strong(store.title, 13.5, if chosen { theme::GOLD } else { theme::TEXT }), label(sub, 11.5, theme::TEXT_FAINT)]
                .spacing(2);
        if store.private {
            words = words.push(label("private: Sinai's conversations", 11.0, theme::GOLD_DIM));
        }
        let readable = shelf.is_some_and(|s| s.plan.is_ok());
        list = list.push(
            button(words)
                .width(Length::Fill)
                .padding([8.0, 10.0])
                .on_press_maybe(readable.then_some(Msg::Store(i)))
                .style(theme::list_row(chosen)),
        );
    }
    // No Look again: every file listed is watched while the page is open.
    list.push(crate::live::badge(&state.shelf_fresh)).into()
}

fn main(state: &State, phase: f32) -> El<'_> {
    let Some(i) = state.store else {
        return container(label("Choose a database on the left.", 14.0, theme::TEXT_DIM)).width(Length::Fill).into();
    };
    let store = &STORES[i];
    let mut body = Column::new()
        .spacing(14)
        .width(Length::Fill)
        .push(column![
            row![strong(store.title, 20.0, theme::TEXT), space().width(Length::Fill), crate::live::badge(&state.store_fresh)]
                .align_y(Alignment::Center),
            label(store.what, 13.0, theme::TEXT_DIM)
        ]
        .spacing(4));
    if store.private {
        body = body.push(ui::notice(
            "Sinai's private conversations, shown on this PC at your word. Read-only, and nothing is sent anywhere.",
            theme::GOLD_DIM,
        ));
    }
    match &state.overview {
        None => return body.push(ui::working(phase, "Reading its tables…")).into(),
        Some(Err(why)) => return body.push(ui::notice(format!("It could not be read: {why}"), theme::CAUTION)).into(),
        Some(Ok(overview)) => {
            if let Some(access) = overview.access {
                body = body.push(note(format!("Opened {}.", access.words())));
            }
            let mut tables = Row::new().spacing(6);
            for t in &overview.tables {
                let on = state.table.as_deref() == Some(t.name.as_str());
                tables = tables.push(
                    button(
                        row![
                            label(t.name.as_str(), 12.5, if on { theme::GOLD } else { theme::TEXT }),
                            label(format!("{}", t.rows), 11.5, theme::TEXT_FAINT)
                        ]
                        .spacing(6),
                    )
                    .padding([5.0, 10.0])
                    .on_press(Msg::Table(t.name.clone()))
                    .style(theme::segment_button(on)),
                );
            }
            body = body.push(tables.wrap());
        }
    }
    if state.table.is_none() {
        return body.push(label("Choose a table.", 13.5, theme::TEXT_DIM)).into();
    }
    body = body.push(
        row![
            text_input("Rows with these words in any column", &state.filter)
                .on_input(Msg::Filter)
                .on_submit(Msg::Search)
                .padding([7.0, 10.0])
                .size(13.0)
                .style(ui::input_style)
                .width(Length::Fill),
            ui::secondary("Find", (!state.loading).then_some(Msg::Search)),
        ]
        .spacing(8)
        .align_y(Alignment::Center),
    );
    match &state.page {
        None => body.push(ui::working(phase, "Reading its rows…")).into(),
        Some(Err(why)) => body.push(ui::notice(format!("Its rows could not be read: {why}"), theme::CAUTION)).into(),
        Some(Ok(page)) => {
            let first = page.offset + 1;
            let last = page.offset + page.rows.len() as i64;
            let of = match page.matching {
                Some(n) => format!("{n} matching \u{201c}{}\u{201d}", state.filtered),
                None => {
                    let total = match &state.overview {
                        Some(Ok(o)) => o.tables.iter().find(|t| t.name == page.table).map(|t| t.rows).unwrap_or(0),
                        _ => 0,
                    };
                    format!("{total} in all")
                }
            };
            let order = if page.newest_first { "newest first" } else { "in stored order" };
            let shown = if page.rows.is_empty() { format!("No rows ({of}).") } else { format!("Rows {first} to {last} of {of}, {order}.") };
            let mut paging = row![label(shown, 12.5, theme::TEXT_DIM), space().width(Length::Fill)].spacing(8).align_y(Alignment::Center);
            if state.loading {
                paging = paging.push(crate::spinner::spinner(phase, 14.0));
            }
            paging = paging
                .push(ui::secondary("Newer", (page.offset > 0 && !state.loading).then_some(Msg::Newer)))
                .push(ui::secondary("Older", (page.rows.len() as i64 == PAGE && !state.loading).then_some(Msg::Older)));
            body = body.push(paging).push(grid(page));
            if let Some(row) = state.detail.and_then(|d| page.rows.get(d)) {
                let mut whole = Column::new().spacing(8);
                for (name, cell) in page.columns.iter().zip(row) {
                    let value = match cell {
                        Cell::Text(_, true) => format!("{}\n(the first {} characters)", cell.show(), super::stores::TEXT_KEPT),
                        _ => cell.show_in(name),
                    };
                    whole = whole.push(column![label(name.as_str(), 12.0, theme::TEXT_DIM), mono(value, 12.5, theme::TEXT)].spacing(2));
                }
                body = body.push(card("The row, whole", whole.push(ui::secondary("Close", Some(Msg::Detail(None))))));
            }
            body.into()
        }
    }
}

/// The rows as a grid: one column per table column, scrolled both ways; a row opens whole when pressed.
fn grid(page: &super::stores::Page) -> El<'_> {
    let mut head = Row::new().spacing(0);
    for name in &page.columns {
        head = head.push(container(strong(ui::cut(name, 28), 12.0, theme::GOLD)).width(CELL_W).padding([6.0, 8.0]));
    }
    let mut rows = Column::new().spacing(0).push(head);
    for (i, cells) in page.rows.iter().enumerate() {
        let mut line = Row::new().spacing(0);
        for (cell, name) in cells.iter().zip(&page.columns) {
            let color = match cell {
                Cell::Null | Cell::Blob(_) => theme::TEXT_FAINT,
                Cell::Hidden => theme::GOLD_DIM,
                _ => theme::TEXT,
            };
            let shown = ui::cut(&cell.show_in(name).replace('\n', " "), CELL_CHARS);
            line = line.push(container(label(shown, 12.0, color)).width(CELL_W).padding([5.0, 8.0]));
        }
        rows = rows.push(button(line).padding(0).on_press(Msg::Detail(Some(i))).style(theme::list_row(false)));
    }
    container(
        scrollable(rows)
            .direction(Direction::Both { vertical: Scrollbar::default(), horizontal: Scrollbar::default() })
            .style(theme::scrollbars)
            .height(Length::Fixed(480.0)),
    )
    .padding(4)
    .style(theme::panel)
    .width(Length::Fill)
    .into()
}
