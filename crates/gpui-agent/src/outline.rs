//! Outline snapshots: a subtree of a [`UiTree`] and/or a depth cap.
//!
//! A full snapshot of a large window costs an agent thousands of tokens
//! when it only needs the top levels or one panel. [`outline`] cuts the tree
//! in the client, after the host has answered a plain `snapshot`, so it works
//! with every host and needs no wire change. A node whose children were cut
//! by the depth cap reports how many it had in `children_count`; the agent
//! can then ask again with that node as the root.

use std::collections::{BTreeMap, BTreeSet};

use serde::Serialize;

use crate::protocol::PlatformKind;
use crate::tree::{UiNode, UiTree};

/// What part of a snapshot to keep.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct OutlineOptions {
    /// Keep only the subtree of the first node with this id (the node
    /// itself included). `None` keeps every root.
    pub root: Option<String>,
    /// Keep this many levels, counted from the roots (or from `root`):
    /// `1` keeps the roots alone. `None` keeps every level.
    pub depth: Option<usize>,
}

impl OutlineOptions {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn root(mut self, id: impl Into<String>) -> Self {
        self.root = Some(id.into());
        self
    }

    pub fn depth(mut self, levels: usize) -> Self {
        self.depth = Some(levels);
        self
    }

    /// `true` when the options keep the whole tree.
    pub fn is_full(&self) -> bool {
        self.root.is_none() && self.depth.is_none()
    }
}

/// One node of an [`Outline`]. It serializes like a [`UiNode`], plus
/// `children_count` when the depth cap cut its children.
#[derive(Debug, Clone, Serialize)]
#[non_exhaustive]
pub struct OutlineNode {
    /// The node, without its children (they are in [`Self::children`]).
    #[serde(flatten)]
    pub node: UiNode,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<OutlineNode>,
    /// Number of direct children the depth cap left out. `None` when
    /// nothing was cut below this node.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub children_count: Option<usize>,
}

/// A cut snapshot. It serializes like a [`UiTree`].
#[derive(Debug, Clone, Serialize)]
#[non_exhaustive]
pub struct Outline {
    pub app: String,
    pub platform: PlatformKind,
    pub ready: bool,
    pub nodes: Vec<OutlineNode>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rev: Option<u64>,
}

/// Cuts `tree` as `options` say. Fails when `options.root` names no node or
/// `options.depth` is `0`.
pub fn outline(tree: &UiTree, options: &OutlineOptions) -> Result<Outline, String> {
    if options.depth == Some(0) {
        return Err("depth must be at least 1".into());
    }
    let levels = options.depth.unwrap_or(usize::MAX);
    let nodes = match &options.root {
        Some(id) => {
            let node = tree
                .find(id)
                .ok_or_else(|| format!("no node `{id}` in the snapshot"))?;
            vec![cut(node, levels)]
        }
        None => tree.nodes.iter().map(|node| cut(node, levels)).collect(),
    };
    Ok(Outline {
        app: tree.app.clone(),
        platform: tree.platform,
        ready: tree.ready,
        nodes,
        rev: options.is_full().then_some(tree.rev).flatten(),
    })
}

fn cut(node: &UiNode, levels: usize) -> OutlineNode {
    let head = head_of(node);
    let (children, children_count) = if levels > 1 {
        let kids = node
            .children
            .iter()
            .map(|child| cut(child, levels - 1))
            .collect();
        (kids, None)
    } else if node.children.is_empty() {
        (Vec::new(), None)
    } else {
        (Vec::new(), Some(node.children.len()))
    };
    OutlineNode {
        node: head,
        children,
        children_count,
    }
}

impl Outline {
    /// The outline in the `format:"compact"` text form
    /// ([`crate::diff::compact_lines`]); a cut node carries
    /// `children_count=N` among its flags.
    pub fn compact_lines(&self) -> String {
        let mut out = String::new();
        for node in &self.nodes {
            compact_node(node, 0, &mut out);
        }
        out
    }

    /// Drop the fingerprints of nodes the outline cut, so a snapshot's
    /// `fingerprints` map stays as small as its tree.
    pub fn retain_fingerprints(&self, fingerprints: &mut BTreeMap<String, u64>) {
        fn ids<'a>(nodes: &'a [OutlineNode], kept: &mut BTreeSet<&'a str>) {
            for node in nodes {
                kept.insert(node.node.id.as_str());
                ids(&node.children, kept);
            }
        }
        let mut kept = BTreeSet::new();
        ids(&self.nodes, &mut kept);
        fingerprints.retain(|id, _| kept.contains(id.as_str()));
    }
}

fn compact_node(node: &OutlineNode, depth: usize, out: &mut String) {
    if !node.node.visible {
        return;
    }
    crate::diff::compact_line(&node.node, depth, node.children_count, out);
    for child in &node.children {
        compact_node(child, depth + 1, out);
    }
}

/// `node` without its children, so a big subtree is not copied only to be
/// dropped.
fn head_of(node: &UiNode) -> UiNode {
    UiNode {
        id: node.id.clone(),
        role: node.role.clone(),
        name: node.name.clone(),
        value: node.value.clone(),
        checked: node.checked,
        enabled: node.enabled,
        focused: node.focused,
        visible: node.visible,
        bounds: node.bounds,
        states: node.states.clone(),
        children: Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tree() -> UiTree {
        let leaf = |id: &str| UiNode::new(id, "button", id);
        let mut panel = UiNode::new("panel", "group", "Panel");
        panel.children = vec![leaf("a"), leaf("b"), leaf("c")];
        let mut list = UiNode::new("list", "list", "List");
        let mut row = UiNode::new("row", "listitem", "Row");
        row.children = vec![leaf("x")];
        list.children = vec![row];
        let mut window = UiNode::new("window", "window", "Todo");
        window.children = vec![panel, list];
        UiTree {
            app: "todo".into(),
            platform: PlatformKind::Headless,
            ready: true,
            nodes: vec![window],
            rev: Some(7),
        }
    }

    #[test]
    fn full_options_keep_the_tree_and_serialize_like_it() {
        let tree = tree();
        let full = outline(&tree, &OutlineOptions::new()).unwrap();
        assert_eq!(
            serde_json::to_value(&full).unwrap(),
            serde_json::to_value(&tree).unwrap()
        );
        assert_eq!(full.compact_lines(), crate::diff::compact_lines(&tree));
    }

    #[test]
    fn depth_cuts_levels_and_counts_the_cut_children() {
        let one = outline(&tree(), &OutlineOptions::new().depth(1)).unwrap();
        assert_eq!(one.rev, None);
        assert!(one.nodes[0].children.is_empty());
        assert_eq!(one.nodes[0].children_count, Some(2));

        let two = outline(&tree(), &OutlineOptions::new().depth(2)).unwrap();
        let window = &two.nodes[0];
        assert_eq!(window.children_count, None);
        assert_eq!(window.children[0].children_count, Some(3));
        assert_eq!(window.children[1].children_count, Some(1));

        let json = serde_json::to_value(&two).unwrap();
        assert_eq!(json["nodes"][0]["children"][0]["children_count"], 3);
        assert!(json["nodes"][0]["children"][0].get("children").is_none());
        assert!(json["nodes"][0].get("children_count").is_none());
        assert_eq!(json["rev"], 7);

        let compact = two.compact_lines();
        assert_eq!(
            compact,
            "window window \"Todo\"\n  panel group \"Panel\"  [children_count=3]\n  list list \"List\"  [children_count=1]\n"
        );
    }

    #[test]
    fn leaves_at_the_cap_report_no_count() {
        let three = outline(&tree(), &OutlineOptions::new().depth(3)).unwrap();
        let panel = &three.nodes[0].children[0];
        assert!(panel.children.iter().all(|c| c.children_count.is_none()));
        assert_eq!(
            three.nodes[0].children[1].children[0].children_count,
            Some(1)
        );
    }

    #[test]
    fn root_keeps_one_subtree_and_combines_with_depth() {
        let list = outline(&tree(), &OutlineOptions::new().root("list")).unwrap();
        assert_eq!(list.nodes.len(), 1);
        assert_eq!(list.nodes[0].node.id, "list");
        assert_eq!(list.nodes[0].children[0].children[0].node.id, "x");

        let capped = outline(&tree(), &OutlineOptions::new().root("list").depth(1)).unwrap();
        assert_eq!(capped.nodes[0].children_count, Some(1));
        assert_eq!(
            capped.compact_lines(),
            "list list \"List\"  [children_count=1]\n"
        );
    }

    #[test]
    fn bad_options_are_refused() {
        let err = outline(&tree(), &OutlineOptions::new().root("nope")).unwrap_err();
        assert!(err.contains("nope"), "{err}");
        let err = outline(&tree(), &OutlineOptions::new().depth(0)).unwrap_err();
        assert!(err.contains("at least 1"), "{err}");
    }

    #[test]
    fn fingerprints_of_cut_nodes_are_dropped() {
        let cut = outline(&tree(), &OutlineOptions::new().root("panel").depth(1)).unwrap();
        let mut fps: BTreeMap<String, u64> = [("window", 1), ("panel", 2), ("a", 3)]
            .into_iter()
            .map(|(id, fp)| (id.to_string(), fp))
            .collect();
        cut.retain_fingerprints(&mut fps);
        assert_eq!(
            fps.into_iter().collect::<Vec<_>>(),
            [("panel".to_string(), 2)]
        );
    }
}
