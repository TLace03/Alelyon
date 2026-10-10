//! The Explorer's folder tree, built from the folder's listing: the paths lattice-core lists for its file picker (git's
//! non-ignored files, or `.latticeignore`'s for a folder without git, each a derived path with forward slashes). The
//! disk is never walked again here, so a file the agent may not read is not offered either.

use std::collections::HashSet;

/// A folder or a file of the tree.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Node {
    pub name: String,
    /// The derived path (`""` for the root).
    pub path: String,
    pub is_dir: bool,
    /// Folders first, then files, each in natural order without regard to case.
    pub children: Vec<Node>,
}

/// One row of the tree as it is drawn.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Row {
    pub depth: u16,
    pub name: String,
    pub path: String,
    pub is_dir: bool,
    pub open: bool,
}

/// Build the tree of `paths` (derived, `/`-separated; a path listed twice, or with an empty part, counts once or not
/// at all).
pub fn build<'a>(paths: impl IntoIterator<Item = &'a str>) -> Node {
    let mut root = Node { name: String::new(), path: String::new(), is_dir: true, children: Vec::new() };
    for path in paths {
        if path.is_empty() || path.split('/').any(str::is_empty) {
            continue;
        }
        let mut node = &mut root;
        let parts: Vec<&str> = path.split('/').collect();
        for (n, part) in parts.iter().enumerate() {
            let is_dir = n + 1 < parts.len();
            let at = node.children.iter().position(|c| c.name == *part && c.is_dir == is_dir);
            let at = match at {
                Some(at) => at,
                None => {
                    let path = parts[..=n].join("/");
                    node.children.push(Node { name: (*part).to_string(), path, is_dir, children: Vec::new() });
                    node.children.len() - 1
                }
            };
            node = &mut node.children[at];
        }
    }
    sort(&mut root);
    root
}

fn sort(node: &mut Node) {
    node.children.sort_by(|a, b| b.is_dir.cmp(&a.is_dir).then_with(|| natural(&a.name, &b.name)));
    for child in &mut node.children {
        sort(child);
    }
}

/// Natural order without regard to case: `file2` before `file10`.
pub fn natural(a: &str, b: &str) -> std::cmp::Ordering {
    use std::cmp::Ordering;
    let (mut x, mut y) = (a.chars().peekable(), b.chars().peekable());
    loop {
        match (x.peek().copied(), y.peek().copied()) {
            (None, None) => return a.cmp(b),
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(p), Some(q)) if p.is_ascii_digit() && q.is_ascii_digit() => {
                let mut m = String::new();
                while let Some(c) = x.peek().copied().filter(char::is_ascii_digit) {
                    m.push(c);
                    x.next();
                }
                let mut n = String::new();
                while let Some(c) = y.peek().copied().filter(char::is_ascii_digit) {
                    n.push(c);
                    y.next();
                }
                let (m, n) = (m.trim_start_matches('0'), n.trim_start_matches('0'));
                let order = m.len().cmp(&n.len()).then_with(|| m.cmp(n));
                if order != Ordering::Equal {
                    return order;
                }
            }
            (Some(p), Some(q)) => {
                let order = p.to_lowercase().cmp(q.to_lowercase());
                if order != Ordering::Equal {
                    return order;
                }
                x.next();
                y.next();
            }
        }
    }
}

/// The rows on show: the root's children, and each open folder's, in order.
pub fn rows(root: &Node, open: &HashSet<String>) -> Vec<Row> {
    let mut out = Vec::new();
    fn walk(node: &Node, depth: u16, open: &HashSet<String>, out: &mut Vec<Row>) {
        for child in &node.children {
            let is_open = child.is_dir && open.contains(&child.path);
            out.push(Row { depth, name: child.name.clone(), path: child.path.clone(), is_dir: child.is_dir, open: is_open });
            if is_open {
                walk(child, depth + 1, open, out);
            }
        }
    }
    walk(root, 0, open, &mut out);
    out
}

/// The folders that hold `path`, outermost first: opening them all shows the file.
pub fn folders_of(path: &str) -> Vec<String> {
    let parts: Vec<&str> = path.split('/').collect();
    (1..parts.len()).map(|n| parts[..n].join("/")).collect()
}

/// The number of files at or below `node`.
pub fn files_in(node: &Node) -> usize {
    if node.is_dir { node.children.iter().map(files_in).sum() } else { 1 }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_listing_becomes_folders_first_in_natural_order() {
        let root = build(["src/main.rs", "src/lattice/mod.rs", "README.md", "src/file10.rs", "src/file2.rs", "Cargo.toml"]);
        let names: Vec<_> = root.children.iter().map(|c| (c.name.as_str(), c.is_dir)).collect();
        assert_eq!(names, vec![("src", true), ("Cargo.toml", false), ("README.md", false)]);
        let src: Vec<_> = root.children[0].children.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(src, vec!["lattice", "file2.rs", "file10.rs", "main.rs"]);
        assert_eq!(root.children[0].children[0].children[0].path, "src/lattice/mod.rs");
        assert_eq!(files_in(&root), 6);
    }

    #[test]
    fn only_open_folders_show_their_rows() {
        let root = build(["a/b/c.txt", "a/d.txt", "e.txt"]);
        let shut = rows(&root, &HashSet::new());
        assert_eq!(shut.iter().map(|r| r.path.as_str()).collect::<Vec<_>>(), vec!["a", "e.txt"]);
        let open: HashSet<String> = ["a".to_string(), "a/b".to_string()].into();
        let shown = rows(&root, &open);
        assert_eq!(
            shown.iter().map(|r| (r.path.as_str(), r.depth)).collect::<Vec<_>>(),
            vec![("a", 0), ("a/b", 1), ("a/b/c.txt", 2), ("a/d.txt", 1), ("e.txt", 0)]
        );
        assert!(shown[0].open && !shown[4].open);
    }

    #[test]
    fn a_file_is_revealed_by_opening_its_folders_and_odd_paths_are_left_out() {
        assert_eq!(folders_of("a/b/c.txt"), vec!["a".to_string(), "a/b".to_string()]);
        assert!(folders_of("top.txt").is_empty());
        let root = build(["", "x//y", "/abs", "ok.txt", "ok.txt"]);
        assert_eq!(root.children.len(), 1);
        assert_eq!(root.children[0].path, "ok.txt");
    }

    #[test]
    fn natural_order_compares_numbers_by_value_and_ignores_case() {
        let mut v = vec!["b10", "B2", "a", "b1", "b01"];
        v.sort_by(|a, b| natural(a, b));
        assert_eq!(v, vec!["a", "b01", "b1", "B2", "b10"]);
    }
}
