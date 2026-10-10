//! Sinai's views in docks: the Angel window's layout format, read into iced's pane grid and written back
//! whenever the person rearranges it.
//!
//! The format is `alelyon.angel.dock` version 1 (the Angel window's own dock module): a tree of
//! splits, each "horizontal" (side by side) or "vertical" (one above another) with a weight per child, ending in
//! groups of views shown as tabs. Every one of the six views is placed exactly once. CENTCOM opens with the
//! default layout (`assets/default-dock.json`, a copy of the Angel window's
//! `dock-templates/current-2026-09-28.json`) and keeps its own copy, `~/.alelyon/angel/centcom-dock.json`, so
//! rearranging CENTCOM never changes the Angel window's.

use std::path::PathBuf;

use iced::widget::pane_grid::{self, Axis, Configuration, Node};
use serde_json::{Value, json};

pub const SCHEMA: &str = "alelyon.angel.dock";
const DEFAULT: &str = include_str!("../assets/default-dock.json");

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum View {
    Angel,
    Stage,
    Actions,
    Machine,
    Waiting,
    Grants,
}

impl View {
    pub const ALL: [View; 6] = [View::Angel, View::Stage, View::Actions, View::Machine, View::Waiting, View::Grants];

    pub fn title(self) -> &'static str {
        match self {
            View::Angel => "Sinai",
            View::Stage => "Stage",
            View::Actions => "Actions",
            View::Machine => "The Machine",
            View::Waiting => "Waiting on you",
            View::Grants => "Grants",
        }
    }

    /// The format's name for the view (the Angel window's).
    fn key(self) -> &'static str {
        match self {
            View::Angel => "angel",
            View::Stage => "stage",
            View::Actions => "actions",
            View::Machine => "machine",
            View::Waiting => "waiting",
            View::Grants => "grants",
        }
    }

    fn from_key(key: &str) -> Option<View> {
        View::ALL.into_iter().find(|v| v.key() == key)
    }
}

/// One dock: its views, shown as tabs, and which one is showing.
#[derive(Clone, Debug, PartialEq)]
pub struct Tabs {
    pub views: Vec<View>,
    pub active: usize,
}

impl Tabs {
    pub fn showing(&self) -> View {
        self.views[self.active.min(self.views.len() - 1)]
    }
}

/// Where CENTCOM keeps its own layout.
pub fn layout_file() -> Option<PathBuf> {
    std::env::var_os("USERPROFILE").map(|home| PathBuf::from(home).join(".alelyon").join("angel").join("centcom-dock.json"))
}

/// The layout to open with: the saved one if it reads, else the default; with why the saved one did not.
pub fn load() -> (Configuration<Tabs>, Option<String>) {
    let default = || parse(&serde_json::from_str(DEFAULT).expect("the default layout is JSON")).expect("the default layout is valid");
    let Some(path) = layout_file().filter(|p| p.is_file()) else {
        return (default(), None);
    };
    let read = std::fs::read_to_string(&path).map_err(|e| e.to_string()).and_then(|t| serde_json::from_str::<Value>(&t).map_err(|e| e.to_string())).and_then(|v| parse(&v));
    match read {
        Ok(config) => (config, None),
        Err(why) => (default(), Some(format!("Your saved layout ({}) could not be used, so the default is shown: {why}", path.display()))),
    }
}

/// A layout document, checked: the schema and version, every view known, and each of the six placed once.
pub fn parse(doc: &Value) -> Result<Configuration<Tabs>, String> {
    if doc.get("schema").and_then(Value::as_str) != Some(SCHEMA) || doc.get("version").and_then(Value::as_u64) != Some(1) {
        return Err(format!("it is not an {SCHEMA} version 1 layout"));
    }
    let root = doc.get("root").ok_or("it has no root")?;
    let mut seen = Vec::new();
    let config = node(root, &mut seen)?;
    for view in View::ALL {
        match seen.iter().filter(|v| **v == view).count() {
            1 => {}
            0 => return Err(format!("{} is not placed", view.title())),
            _ => return Err(format!("{} is placed more than once", view.title())),
        }
    }
    Ok(config)
}

fn node(v: &Value, seen: &mut Vec<View>) -> Result<Configuration<Tabs>, String> {
    if let Some(list) = v.get("views").and_then(Value::as_array) {
        let views = list
            .iter()
            .map(|k| k.as_str().and_then(View::from_key).ok_or_else(|| format!("{k} is not a view")))
            .collect::<Result<Vec<View>, String>>()?;
        if views.is_empty() {
            return Err("a dock has no views".into());
        }
        seen.extend(&views);
        let active = v.get("active").and_then(Value::as_u64).unwrap_or(0) as usize;
        return Ok(Configuration::Pane(Tabs { active: active.min(views.len() - 1), views }));
    }
    let children = v.get("children").and_then(Value::as_array).ok_or("a split has no children")?;
    if children.is_empty() {
        return Err("a split has no children".into());
    }
    // The format's "horizontal" puts its children side by side: iced's vertical split line.
    let axis = match v.get("orientation").and_then(Value::as_str) {
        Some("horizontal") => Axis::Vertical,
        Some("vertical") => Axis::Horizontal,
        other => return Err(format!("{other:?} is not an orientation")),
    };
    let weights: Vec<f32> = match v.get("weights").and_then(Value::as_array) {
        Some(w) if w.len() == children.len() => w.iter().map(|x| x.as_f64().unwrap_or(1.0).max(1.0) as f32).collect(),
        _ => vec![1.0; children.len()],
    };
    let parts = children.iter().map(|c| node(c, seen)).collect::<Result<Vec<_>, String>>()?;
    Ok(fold(axis, parts, &weights))
}

/// Children with weights, as iced's two-way splits: the first against the rest, and so on.
fn fold(axis: Axis, mut parts: Vec<Configuration<Tabs>>, weights: &[f32]) -> Configuration<Tabs> {
    if parts.len() == 1 {
        return parts.pop().expect("one part");
    }
    let total: f32 = weights.iter().sum();
    let first = parts.remove(0);
    Configuration::Split { axis, ratio: weights[0] / total, a: Box::new(first), b: Box::new(fold(axis, parts, &weights[1..])) }
}

/// The grid's layout as a document in the same format (splits two ways, as the grid holds them).
pub fn to_json(state: &pane_grid::State<Tabs>) -> Value {
    fn walk(node: &Node, state: &pane_grid::State<Tabs>) -> Value {
        match node {
            Node::Pane(pane) => {
                let tabs = state.get(*pane).cloned().unwrap_or(Tabs { views: vec![View::Angel], active: 0 });
                json!({"views": tabs.views.iter().map(|v| v.key()).collect::<Vec<_>>(), "active": tabs.active})
            }
            Node::Split { axis, ratio, a, b, .. } => {
                let orientation = if *axis == Axis::Vertical { "horizontal" } else { "vertical" };
                let first = (ratio * 1000.0).round().clamp(1.0, 999.0);
                json!({"orientation": orientation, "weights": [first, 1000.0 - first], "children": [walk(a, state), walk(b, state)]})
            }
        }
    }
    json!({"schema": SCHEMA, "version": 1, "root": walk(state.layout(), state)})
}

/// Write the grid's layout to CENTCOM's own file.
pub fn save(state: &pane_grid::State<Tabs>) -> Result<(), String> {
    let path = layout_file().ok_or("USERPROFILE is not set")?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let text = serde_json::to_string_pretty(&to_json(state)).map_err(|e| e.to_string())?;
    std::fs::write(&path, text).map_err(|e| format!("{}: {e}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn views_in(config: &Configuration<Tabs>, out: &mut Vec<View>) {
        match config {
            Configuration::Pane(t) => out.extend(&t.views),
            Configuration::Split { a, b, .. } => {
                views_in(a, out);
                views_in(b, out);
            }
        }
    }

    #[test]
    fn the_default_layout_opens_with_sinai_upper_left_and_every_view_once() {
        let config = parse(&serde_json::from_str(DEFAULT).unwrap()).unwrap();
        let mut views = Vec::new();
        views_in(&config, &mut views);
        assert_eq!(views, [View::Angel, View::Machine, View::Grants, View::Stage, View::Actions, View::Waiting]);
        // the root puts the two columns side by side, the left one narrower (333 of 1360)
        match &config {
            Configuration::Split { axis, ratio, .. } => {
                assert_eq!(*axis, Axis::Vertical);
                assert!((ratio - 333.0 / 1360.0).abs() < 1e-4, "{ratio}");
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn a_layout_that_breaks_the_rules_is_refused_with_its_reason() {
        let ok = serde_json::from_str::<Value>(DEFAULT).unwrap();
        let mut twice = ok.clone();
        twice["root"]["children"][0]["children"][0]["views"] = json!(["angel", "stage"]);
        assert!(parse(&twice).unwrap_err().contains("more than once"));
        let mut unknown = ok.clone();
        unknown["root"]["children"][0]["children"][0]["views"] = json!(["markets"]);
        assert!(parse(&unknown).unwrap_err().contains("not a view"));
        assert!(parse(&json!({"schema": "x", "version": 1})).is_err());
        let mut missing = ok;
        missing["root"]["children"][0]["children"][0]["views"] = json!(["grants"]);
        assert!(parse(&missing).is_err(), "Sinai unplaced, Grants twice");
    }

    #[test]
    fn a_layout_written_back_reads_the_same() {
        let config = parse(&serde_json::from_str(DEFAULT).unwrap()).unwrap();
        let state = pane_grid::State::with_configuration(config);
        let written = to_json(&state);
        let again = pane_grid::State::with_configuration(parse(&written).unwrap());
        assert_eq!(to_json(&again), written, "the round trip is exact");
        let mut views = Vec::new();
        views_in(&parse(&written).unwrap(), &mut views);
        assert_eq!(views.len(), 6);
    }
}
