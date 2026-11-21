//! Short refs (`@e1`, `@e2`, …) for the nodes of the last `snapshot --refs`.
//!
//! Host ids can be long or opaque (`n1438`); a ref is short, stable until
//! the next `snapshot --refs`, and accepted wherever a command takes a node
//! id. The table lives in the runtime directory, one file per host address,
//! because each CLI command is its own process.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::fmt::Write as _;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};

use gpui_agent::outline::{Outline, OutlineNode};
use gpui_agent::tree::UiNode;
use serde::{Deserialize, Serialize};

/// What a ref stands for: the node id and, when the host reported one, the
/// node's fingerprint at the time of the `snapshot --refs` (for `--guard`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Saved {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fp: Option<u64>,
}

/// A saved entry: tables written before fingerprints hold the bare id.
#[derive(Deserialize)]
#[serde(untagged)]
enum Stored {
    Id(String),
    Saved(Saved),
}

pub(crate) type Table = BTreeMap<String, Saved>;

fn table_path(root: &Path, addr: SocketAddr) -> PathBuf {
    let name: String = addr
        .to_string()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect();
    root.join("refs").join(format!("{name}.json"))
}

/// `true` for an argument spelled as a ref.
pub(crate) fn is_ref(arg: &str) -> bool {
    arg.strip_prefix("@e")
        .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
}

fn ref_arg(arg: &str) -> Option<(&str, &str)> {
    if is_ref(arg) {
        return Some(("", arg));
    }
    let (option, value) = arg.split_once('=')?;
    (NODE_ID_OPTIONS.contains(&option) && is_ref(value)).then_some((&arg[..=option.len()], value))
}

const NODE_ID_OPTIONS: &[&str] = &[
    "--id",
    "--target",
    "--from",
    "--to",
    "--root",
    "--wait-for",
    "--wait-for-gone",
];

/// `true` for an exact ref argument or a node-id option with a ref value.
pub(crate) fn has_ref(arg: &str) -> bool {
    ref_arg(arg).is_some()
}

/// A node earns a line when it says something or can be acted on; bare
/// layout containers only pass their children up. Where the host gives
/// geometry (`sized`), a node without any is not on screen. A node whose
/// children an outline cut is always listed, so its ref can be the next
/// `--root`.
#[expect(
    clippy::fn_params_excessive_bools,
    reason = "sized is the headless-bounds flag"
)]
fn listed(node: &OutlineNode, sized: bool) -> bool {
    let shown = &node.node;
    shown.visible
        && (node.children_count.is_some()
            || ((!sized || has_size(shown))
                && (shown.role != "generic" || !shown.name.is_empty() || shown.value.is_some())))
}

fn has_size(node: &UiNode) -> bool {
    node.bounds.w > 0.0 && node.bounds.h > 0.0
}

/// Headless hosts publish no bounds at all.
fn any_sized(nodes: &[OutlineNode]) -> bool {
    nodes
        .iter()
        .any(|node| has_size(&node.node) || any_sized(&node.children))
}

struct Render<'a> {
    sized: bool,
    fingerprints: &'a BTreeMap<String, u64>,
    show_fingerprints: bool,
    out: String,
    table: Table,
}

impl Render<'_> {
    fn nodes(&mut self, nodes: &[OutlineNode], depth: usize) {
        for node in nodes {
            if !node.node.visible {
                continue;
            }
            let mut below = depth;
            if listed(node, self.sized) {
                self.line(node, depth);
                below += 1;
            }
            self.nodes(&node.children, below);
        }
    }

    fn line(&mut self, outline: &OutlineNode, depth: usize) {
        let node = &outline.node;
        let out = &mut self.out;
        let key = format!("e{}", self.table.len() + 1);
        let _ = write!(
            out,
            "{:indent$}@{key} {}",
            "",
            node.role,
            indent = depth * 2
        );
        if !node.name.is_empty() {
            let _ = write!(out, " {:?}", node.name);
        }
        if let Some(value) = &node.value {
            let _ = write!(out, " value={value:?}");
        }
        if let Some(checked) = node.checked {
            let _ = write!(out, " checked={checked}");
        }
        if !node.enabled {
            out.push_str(" disabled");
        }
        if node.focused {
            out.push_str(" focused");
        }
        if let Some(count) = outline.children_count {
            let _ = write!(out, " children_count={count}");
        }
        let fp = self.fingerprints.get(&node.id).copied();
        if let Some(fp) = fp.filter(|_| self.show_fingerprints) {
            let _ = write!(out, " fp={fp}");
        }
        out.push('\n');
        self.table.insert(
            key,
            Saved {
                id: node.id.clone(),
                fp,
            },
        );
    }
}

/// The text of `outline` and the ref table that goes with it. Each ref
/// keeps its node's entry in `fingerprints`; `show_fingerprints` also
/// prints it on the line (`fp=…`).
#[expect(
    clippy::fn_params_excessive_bools,
    reason = "show_fingerprints mirrors --fingerprints"
)]
pub(crate) fn render(
    outline: &Outline,
    fingerprints: &BTreeMap<String, u64>,
    show_fingerprints: bool,
) -> (String, Table) {
    let mut render = Render {
        sized: any_sized(&outline.nodes),
        fingerprints,
        show_fingerprints,
        out: String::new(),
        table: Table::new(),
    };
    render.nodes(&outline.nodes, 0);
    (render.out, render.table)
}

pub(crate) fn save(root: &Path, addr: SocketAddr, table: &Table) -> std::io::Result<()> {
    gpui_agent::discovery::ensure_runtime_dir(root)?;
    let path = table_path(root, addr);
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let body = serde_json::to_vec(table)?;
    let mut n = 0u32;
    let (tmp, mut file) = loop {
        let candidate = path.with_extension(format!("{}.{}.tmp", std::process::id(), n));
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&candidate)
        {
            Ok(file) => break (candidate, file),
            Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => {
                n = n.checked_add(1).ok_or(err)?;
            }
            Err(err) => return Err(err),
        }
    };
    let result = (|| {
        std::io::Write::write_all(&mut file, &body)?;
        file.sync_all()?;
        drop(file);
        std::fs::rename(&tmp, &path)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(tmp);
    }
    result
}

/// Replaces every ref among `args` with the node id it stands for, and
/// returns what each replaced ref stood for. The arguments stay as the OS
/// gave them: a path need not be UTF-8.
pub(crate) fn resolve(
    root: &Path,
    addr: SocketAddr,
    args: &mut [OsString],
) -> Result<Vec<Saved>, String> {
    if !args.iter().any(|arg| arg.to_str().is_some_and(has_ref)) {
        return Ok(Vec::new());
    }
    gpui_agent::discovery::ensure_runtime_dir(root)
        .map_err(|error| format!("could not use runtime directory: {error}"))?;
    let table: BTreeMap<String, Stored> = std::fs::read(table_path(root, addr))
        .ok()
        .and_then(|body| serde_json::from_slice(&body).ok())
        .ok_or_else(|| {
            format!("no refs saved for {addr}: run `gpui-agent snapshot --refs` first")
        })?;
    let mut used = Vec::new();
    for arg in args.iter_mut() {
        let Some((prefix, name)) = arg.to_str().and_then(ref_arg) else {
            continue;
        };
        let saved: Saved = match table.get(&name[1..]) {
            Some(Stored::Id(id)) => Saved {
                id: id.clone(),
                fp: None,
            },
            Some(Stored::Saved(saved)) => saved.clone(),
            None => {
                return Err(format!(
                    "unknown ref {name}: it is not in the last `gpui-agent snapshot --refs`"
                ));
            }
        };
        *arg = OsString::from(format!("{prefix}{}", saved.id));
        used.push(saved);
    }
    Ok(used)
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui_agent::outline::{OutlineOptions, outline};
    use gpui_agent::tree::UiTree;

    fn full(tree: &UiTree) -> Outline {
        outline(tree, &OutlineOptions::new()).unwrap()
    }

    fn render_tree(tree: &UiTree) -> (String, Table) {
        render(&full(tree), &BTreeMap::new(), false)
    }

    fn node(id: &str, role: &str, name: &str, children: Vec<UiNode>) -> UiNode {
        let mut node = UiNode::new(id, role, name);
        node.bounds.w = 10.0;
        node.bounds.h = 10.0;
        node.children = children;
        node
    }

    #[test]
    fn refs_number_the_listed_nodes_and_resolve_back_to_ids() {
        let mut hidden = node("gone", "button", "Hidden", vec![]);
        hidden.visible = false;
        let mut tree = UiTree {
            app: "t".into(),
            platform: gpui_agent::protocol::PlatformKind::Headless,
            ready: true,
            nodes: vec![node(
                "root",
                "generic",
                "",
                vec![
                    node("n12", "button", "Save", vec![]),
                    hidden,
                    node(
                        "list",
                        "scroll",
                        "",
                        vec![node("n40", "note", "Row", vec![])],
                    ),
                ],
            )],
            rev: None,
        };
        let (text, table) = render_tree(&tree);
        assert_eq!(
            text,
            "@e1 button \"Save\"\n@e2 scroll\n  @e3 note \"Row\"\n"
        );

        // A headless host gives no bounds: its nodes are listed all the same.
        #[expect(
            clippy::items_after_statements,
            reason = "kept beside the only assertion that uses it for readability"
        )]
        fn unsize(nodes: &mut [UiNode]) {
            for node in nodes {
                node.bounds.w = 0.0;
                node.bounds.h = 0.0;
                unsize(&mut node.children);
            }
        }
        let mut headless = tree.clone();
        unsize(&mut headless.nodes);
        assert_eq!(render_tree(&headless).0, text);
        // With geometry, a node that has none is off screen.
        tree.nodes[0].children[0].bounds.w = 0.0;
        assert_eq!(render_tree(&tree).0, "@e1 scroll\n  @e2 note \"Row\"\n");

        let root = std::env::temp_dir().join(format!("gpui-agent-refs-{}", std::process::id()));
        let addr: SocketAddr = "127.0.0.1:17421".parse().unwrap();
        save(&root, addr, &table).unwrap();
        let mut args: Vec<OsString> = vec![
            "press".into(),
            "@e3".into(),
            "--id=@e1".into(),
            "--target=@e1".into(),
            "--from=@e1".into(),
            "--to=@e1".into(),
            "--name=@e1".into(),
            "--value=@e1".into(),
            "--text=hi @e1".into(),
            "@everyone".into(),
        ];
        let used = resolve(&root, addr, &mut args).unwrap();
        assert_eq!(used.len(), 5);
        assert!(used.iter().all(|saved| saved.fp.is_none()));
        assert_eq!(
            args,
            [
                "press",
                "n40",
                "--id=n12",
                "--target=n12",
                "--from=n12",
                "--to=n12",
                "--name=@e1",
                "--value=@e1",
                "--text=hi @e1",
                "@everyone"
            ]
        );
        let mut stale: Vec<OsString> = vec!["@e9".into()];
        assert!(
            resolve(&root, addr, &mut stale)
                .unwrap_err()
                .contains("unknown ref @e9")
        );
        let other: SocketAddr = "127.0.0.1:1".parse().unwrap();
        assert!(
            resolve(&root, other, &mut stale)
                .unwrap_err()
                .contains("no refs saved")
        );
        save(&root, addr, &BTreeMap::new()).unwrap();
        assert_eq!(std::fs::read(table_path(&root, addr)).unwrap(), b"{}");
        save(&root, addr, &table).unwrap();
        assert_eq!(std::fs::read_dir(root.join("refs")).unwrap().count(), 1);
        let blocked = root.join("not-a-directory");
        std::fs::write(&blocked, b"blocked").unwrap();
        assert!(save(&blocked, addr, &table).is_err());
        let _ = std::fs::remove_dir_all(root);
    }

    fn todo_tree() -> UiTree {
        let mut input = node("todo-input", "textbox", "What?", vec![]);
        input.value = Some("milk".into());
        UiTree {
            app: "t".into(),
            platform: gpui_agent::protocol::PlatformKind::Headless,
            ready: true,
            nodes: vec![node(
                "page",
                "page",
                "Todos",
                vec![
                    input,
                    node(
                        "wrap",
                        "generic",
                        "",
                        vec![node("todo-item-1", "listitem", "Milk", vec![])],
                    ),
                ],
            )],
            rev: None,
        }
    }

    #[test]
    fn refs_keep_fingerprints_and_can_print_them() {
        let tree = todo_tree();
        let fps = tree.fingerprint_map();
        let (quiet, table) = render(&full(&tree), &fps, false);
        assert!(!quiet.contains("fp="), "{quiet}");
        assert_eq!(table["e2"].id, "todo-input");
        assert_eq!(table["e2"].fp, Some(fps["todo-input"]));
        let (loud, _) = render(&full(&tree), &fps, true);
        assert!(
            loud.contains(&format!(
                "@e2 textbox \"What?\" value=\"milk\" fp={}\n",
                fps["todo-input"]
            )),
            "{loud}"
        );

        let root = std::env::temp_dir().join(format!("gpui-agent-refs-fp-{}", std::process::id()));
        let addr: SocketAddr = "127.0.0.1:17423".parse().unwrap();
        save(&root, addr, &table).unwrap();
        let body = std::fs::read_to_string(table_path(&root, addr)).unwrap();
        assert!(
            body.contains(&format!(
                "\"e2\":{{\"id\":\"todo-input\",\"fp\":{}}}",
                fps["todo-input"]
            )),
            "{body}"
        );
        let mut args: Vec<OsString> = vec!["key".into(), "@e2".into(), "--root=@e1".into()];
        let used = resolve(&root, addr, &mut args).unwrap();
        assert_eq!(args, ["key", "todo-input", "--root=page"]);
        assert_eq!(used[0].fp, Some(fps["todo-input"]));

        // A table saved before fingerprints holds bare ids.
        std::fs::write(table_path(&root, addr), br#"{"e1":"page"}"#).unwrap();
        let mut args: Vec<OsString> = vec!["@e1".into(), "--wait-for=@e1".into()];
        let used = resolve(&root, addr, &mut args).unwrap();
        assert_eq!(args, ["page", "--wait-for=page"]);
        assert_eq!(
            used[0],
            Saved {
                id: "page".into(),
                fp: None
            }
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn a_cut_outline_lists_its_cut_nodes_with_their_counts() {
        let tree = todo_tree();
        let cut = outline(&tree, &OutlineOptions::new().depth(2)).unwrap();
        let (text, table) = render(&cut, &BTreeMap::new(), false);
        assert_eq!(
            text,
            "@e1 page \"Todos\"\n  @e2 textbox \"What?\" value=\"milk\"\n  @e3 generic children_count=1\n"
        );
        assert_eq!(table["e3"].id, "wrap");
        let sub = outline(&tree, &OutlineOptions::new().root("wrap")).unwrap();
        assert_eq!(
            render(&sub, &BTreeMap::new(), false).0,
            "@e1 listitem \"Milk\"\n"
        );
    }
}
