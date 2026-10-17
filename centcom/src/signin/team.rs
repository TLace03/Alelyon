//! Settings > Account's Team part: the seats of a Team plan, and the invitations waiting for the person.
//!
//! - **The admin of a Team plan** (the person who pays for it) sees how many seats are taken, each person on it (invited, until a day; or on the
//!   team), and can invite a person by their exact username or remove one. Nobody is put on a team without saying
//!   yes: an invitation holds a seat until the person accepts it, declines it or it lapses.
//! - **A person holding a seat** on somebody's team sees whose, and can leave it.
//! - **Anyone** sees the invitations waiting for them, with Accept and Decline.
//!
//! Everything is asked of the identity service's Team routes (the identity client's `billing` team calls) on a task
//! of its own, with the account's billing round: an answer for an account that has since signed out is dropped.
//! Seats cost nothing to fill or empty here; only the number of seats is billed, and that changes in Manage billing.

use iced::widget::{Space, column, row, text_input};
use iced::{Alignment, Task};

use super::{El, Msg, State, billing, words};
use crate::theme;

/// The Team part's state.
#[derive(Debug, Default)]
pub struct TeamState {
    /// The person's own Team (as its admin), as last asked; None when they own none, or before the first answer.
    pub team: Option<Result<billing::Team, String>>,
    /// The invitations waiting for the person.
    pub invites: Vec<billing::Invite>,
    /// The username typed to invite.
    pub name: String,
    /// A Team call is on its way.
    pub busy: bool,
    /// What the last Team call said, and whether it went wrong.
    pub said: Option<(String, bool)>,
}

/// The Team part's messages.
#[derive(Clone, Debug)]
pub enum TeamMsg {
    /// The person's Team, as the service answered.
    Read(u64, Result<billing::Team, billing::Problem>),
    /// The invitations waiting for the person.
    Invites(u64, Result<Vec<billing::Invite>, billing::Problem>),
    Name(String),
    Invite,
    Remove(String),
    Leave,
    Accept(i64),
    Decline(i64),
    /// A change was made, or why not: its words on success.
    Done(u64, Result<String, billing::Problem>),
}

impl State {
    /// Ask for the person's Team and the invitations waiting for them, alongside the plan.
    pub(super) fn ask_team(&mut self) -> Task<Msg> {
        let Some((c, token)) = self.session() else { return Task::none() };
        let round = self.billing_round;
        let (c2, token2) = (c.clone(), token.clone());
        Task::batch([
            Task::perform(async move { billing::team(&c, &token).await }, move |a| Msg::Team(TeamMsg::Read(round, a))),
            Task::perform(async move { billing::invites(&c2, &token2).await }, move |a| Msg::Team(TeamMsg::Invites(round, a))),
        ])
    }

    pub(super) fn update_team(&mut self, msg: TeamMsg) -> Task<Msg> {
        let round = self.billing_round;
        let current = |r: u64| r == round;
        match msg {
            TeamMsg::Read(r, answer) if current(r) => {
                self.team.team = match answer {
                    Ok(team) => Some(Ok(team)),
                    // Not a Team plan's admin: the part shows no seats.
                    Err(problem) if problem.code() == Some("not-team-owner") => None,
                    Err(problem) => Some(Err(problem.words())),
                };
            }
            TeamMsg::Invites(r, answer) if current(r) => {
                if let Ok(invites) = answer {
                    self.team.invites = invites;
                }
            }
            TeamMsg::Name(name) => self.team.name = name,
            TeamMsg::Done(r, answer) if current(r) => {
                self.team.busy = false;
                self.team.said = Some(match answer {
                    Ok(words) => (words, false),
                    Err(problem) => (problem.words(), true),
                });
                // What changed shows: the seats, the invitations, and the plan (a seat taken or left changes it).
                return Task::batch([self.ask_team(), self.ask_plan()]);
            }
            TeamMsg::Read(..) | TeamMsg::Invites(..) | TeamMsg::Done(..) => {}
            action => return self.team_action(action),
        }
        Task::none()
    }

    fn team_action(&mut self, action: TeamMsg) -> Task<Msg> {
        if self.team.busy {
            return Task::none();
        }
        let Some((c, token)) = self.session() else { return Task::none() };
        let round = self.billing_round;
        let done = move |a| Msg::Team(TeamMsg::Done(round, a));
        let task = match action {
            TeamMsg::Invite => {
                let name = self.team.name.trim().trim_start_matches('@').to_string();
                if name.is_empty() {
                    self.team.said = Some(("Type the username of the person to invite.".into(), true));
                    return Task::none();
                }
                self.team.name.clear();
                Task::perform(
                    async move {
                        billing::invite(&c, &token, &name)
                            .await
                            .map(|m| format!("Invited {}. They are on the team once they accept.", m.name()))
                    },
                    done,
                )
            }
            TeamMsg::Remove(id) => Task::perform(
                async move { billing::remove(&c, &token, &id).await.map(|()| "Removed from the team.".to_string()) },
                done,
            ),
            TeamMsg::Leave => Task::perform(
                async move { billing::leave(&c, &token).await.map(|()| "You left the team.".to_string()) },
                done,
            ),
            TeamMsg::Accept(id) => Task::perform(
                async move { billing::accept(&c, &token, id).await.map(|()| "You joined the team.".to_string()) },
                done,
            ),
            TeamMsg::Decline(id) => Task::perform(
                async move { billing::decline(&c, &token, id).await.map(|()| "Invitation declined.".to_string()) },
                done,
            ),
            _ => return Task::none(),
        };
        self.team.busy = true;
        self.team.said = None;
        task
    }
}

/// A day of an RFC 3339 time (`2026-10-30`).
fn day(at: &Option<String>) -> String {
    at.as_deref().map(|t| t.get(..10).unwrap_or(t).to_string()).unwrap_or_default()
}

/// One person's state on the team, in words.
pub fn member_words(m: &billing::Member) -> String {
    if m.pending() {
        match day(&m.expires_at) {
            d if d.is_empty() => "invited".into(),
            d => format!("invited, until {d}"),
        }
    } else if m.has_access {
        "on the team".into()
    } else {
        "on the team, without a seat now".into()
    }
}

/// The seats in words: "2 of 3 seats taken", and when more people hold seats than are paid for.
pub fn seats_words(t: &billing::Team) -> String {
    let said = format!("{} of {} {} taken", t.used, t.seats, if t.seats == 1 { "seat" } else { "seats" });
    if t.over_limit { format!("{said} · more people than seats: the latest to join have no access until a seat frees") } else { said }
}

fn small<'a>(label: &'a str, msg: Option<Msg>) -> El<'a> {
    super::settings::action(label, msg)
}

/// The Team part: nothing when the person owns no Team, holds no seat and has no invitation.
pub fn view<'a>(s: &'a State) -> El<'a> {
    let t = &s.team;
    let busy = t.busy;
    let gap = || Space::new().width(192);
    let mut col = column![].spacing(6);
    let mut any = false;
    match &t.team {
        Some(Ok(team)) => {
            any = true;
            col = col.push(super::settings::line("Team", seats_words(team), if team.over_limit { theme::CAUTION } else { theme::TEXT }));
            for m in &team.members {
                col = col.push(
                    row![
                        gap(),
                        words(format!("{} · {}", m.name(), member_words(m)), 12.5, theme::TEXT),
                        Space::new().width(8),
                        small(if m.pending() { "Cancel invitation" } else { "Remove" }, (!busy).then(|| Msg::Team(TeamMsg::Remove(m.id.clone())))),
                    ]
                    .align_y(Alignment::Center),
                );
            }
            col = col.push(
                row![
                    gap(),
                    text_input("Username to invite", &t.name)
                        .on_input(|v| Msg::Team(TeamMsg::Name(v)))
                        .on_submit(Msg::Team(TeamMsg::Invite))
                        .width(220)
                        .size(13),
                    Space::new().width(8),
                    small("Invite", (!busy).then_some(Msg::Team(TeamMsg::Invite))),
                ]
                .align_y(Alignment::Center),
            );
        }
        Some(Err(why)) => {
            any = true;
            col = col.push(super::settings::line("Team", why.clone(), theme::CAUTION));
        }
        None => {}
    }
    // A seat on somebody else's team.
    if let Some(Ok(plan)) = &s.plan
        && plan.source.as_deref() == Some("team-seat")
    {
        any = true;
        let whose = plan.team_owner.as_deref().map(|o| format!("@{o}'s team")).unwrap_or_else(|| "a team".into());
        col = col.push(
            row![gap(), words(format!("You hold a seat on {whose}."), 12.5, theme::TEXT), Space::new().width(8), small("Leave", (!busy).then_some(Msg::Team(TeamMsg::Leave)))]
                .align_y(Alignment::Center),
        );
    }
    for i in &t.invites {
        any = true;
        let from = i.from.as_deref().map(|f| format!("@{f}'s team")).unwrap_or_else(|| "a team".into());
        let until = day(&i.expires_at);
        col = col.push(
            row![
                gap(),
                words(format!("Invitation to {from}{}", if until.is_empty() { String::new() } else { format!(", until {until}") }), 12.5, theme::GOLD),
                Space::new().width(8),
                small("Accept", (!busy).then_some(Msg::Team(TeamMsg::Accept(i.invite_id)))),
                small("Decline", (!busy).then_some(Msg::Team(TeamMsg::Decline(i.invite_id)))),
            ]
            .spacing(4)
            .align_y(Alignment::Center),
        );
    }
    if let Some((said, wrong)) = &t.said {
        any = true;
        col = col.push(row![gap(), words(said.as_str(), 12.0, if *wrong { theme::CAUTION } else { theme::TEXT_DIM })]);
    }
    if any { col.into() } else { column![].into() }
}
