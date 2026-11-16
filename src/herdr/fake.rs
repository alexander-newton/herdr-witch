//! `FakeHerdr`: an in-memory herdr for daemon-logic tests. Each tab holds a
//! layout tree like herdr's `LayoutNode`; splitting a pane replaces its leaf
//! with a split whose first child is the old pane and second the new one,
//! exactly as the real server does. Every `HerdrApi` call is logged in
//! `calls`.

use super::api::{HerdrApi, HerdrError, OpenPane, Result};
use super::types::{
    AgentInfo, Dir, LayoutPane, LayoutSnapshot, LayoutSplit, PaneInfo, Rect, TabInfo,
};
use std::collections::HashMap;

#[derive(Clone, Debug, PartialEq)]
pub enum Node {
    Pane(String),
    Split {
        dir: Dir,
        ratio: f32,
        first: Box<Node>,
        second: Box<Node>,
    },
}

#[derive(Clone, Debug)]
pub struct FakeTab {
    pub info: TabInfo,
    pub root: Node,
}

pub struct FakeHerdr {
    /// Tabs in creation order.
    pub tabs: Vec<FakeTab>,
    pub panes: Vec<PaneInfo>,
    pub agents: Vec<AgentInfo>,
    /// The server-wide focused pane.
    pub focused: Option<String>,
    /// Every API call, e.g. `open_plugin_pane sigil w1:p1 right HERDR_WITCH_LINE=3`.
    pub calls: Vec<String>,
    /// Label given to panes opened per entrypoint (the real server uses the
    /// manifest pane `title`); unset entrypoints are labelled with their id.
    pub labels: HashMap<&'static str, String>,
    /// Tab area (width, height) used for `layout` rects.
    pub area: (u16, u16),
    /// Visible text per pane for `pane_text` (missing panes read as "").
    pub texts: HashMap<String, String>,
    /// Foreground argv per pane for `pane_argv`: tab roots run `bash`,
    /// plugin panes `<root>/target/release/herdr-witch <entrypoint>`.
    pub argv: HashMap<String, Vec<String>>,
    /// This plugin in `plugin.list`: `Some(enabled)`, or `None` once
    /// unlinked. Starts installed and enabled.
    pub plugin: Option<bool>,
    /// Injected failures: a call whose log line equals the key returns the
    /// error (after being logged), e.g. `"pane_text w1:p1"`.
    pub fail: HashMap<String, fn() -> HerdrError>,
    /// Workspace labels and how many tabs each has had; `w{index+1}` is the id.
    workspaces: Vec<(String, u32)>,
    next_pane: u32,
}

impl Default for FakeHerdr {
    fn default() -> Self {
        Self::new()
    }
}

fn remote(code: &str, message: String) -> HerdrError {
    HerdrError::Remote {
        code: code.into(),
        message,
    }
}

impl Node {
    fn leaves<'a>(&'a self, out: &mut Vec<&'a str>) {
        match self {
            Node::Pane(id) => out.push(id),
            Node::Split { first, second, .. } => {
                first.leaves(out);
                second.leaves(out);
            }
        }
    }

    fn contains(&self, id: &str) -> bool {
        let mut v = Vec::new();
        self.leaves(&mut v);
        v.contains(&id)
    }

    fn first_leaf(&self) -> &str {
        match self {
            Node::Pane(id) => id,
            Node::Split { first, .. } => first.first_leaf(),
        }
    }

    fn find_leaf(&mut self, id: &str) -> Option<&mut Node> {
        match self {
            Node::Pane(p) if p == id => Some(self),
            Node::Pane(_) => None,
            Node::Split { first, second, .. } => match first.find_leaf(id) {
                Some(n) => Some(n),
                None => second.find_leaf(id),
            },
        }
    }

    /// The first leaf of `id`'s sibling subtree.
    fn sibling_leaf(&self, id: &str) -> Option<&str> {
        let Node::Split { first, second, .. } = self else {
            return None;
        };
        match (first.as_ref(), second.as_ref()) {
            (Node::Pane(p), other) if p == id => Some(other.first_leaf()),
            (other, Node::Pane(p)) if p == id => Some(other.first_leaf()),
            _ => first.sibling_leaf(id).or_else(|| second.sibling_leaf(id)),
        }
    }

    /// Remove leaf `id`, collapsing its parent split; `None` if nothing is left.
    fn remove(self, id: &str) -> Option<Node> {
        match self {
            Node::Pane(p) if p == id => None,
            Node::Pane(p) => Some(Node::Pane(p)),
            Node::Split {
                dir,
                ratio,
                first,
                second,
            } => match (first.remove(id), second.remove(id)) {
                (Some(f), Some(s)) => Some(Node::Split {
                    dir,
                    ratio,
                    first: Box::new(f),
                    second: Box::new(s),
                }),
                (f, s) => f.or(s),
            },
        }
    }

    /// herdr's rects: the first child gets `round(size * ratio)` cells.
    /// Split ids follow herdr's `split_<depth>_<path>` (`root` at depth 0,
    /// then `0`/`1` per step, e.g. `split_1_0`).
    fn rects(
        &self,
        r: Rect,
        path: &mut String,
        panes: &mut Vec<(String, Rect)>,
        splits: &mut Vec<LayoutSplit>,
    ) {
        match self {
            Node::Pane(id) => panes.push((id.clone(), r)),
            Node::Split {
                dir,
                ratio,
                first,
                second,
            } => {
                let id = if path.is_empty() {
                    "split_0_root".to_string()
                } else {
                    format!("split_{}_{path}", path.len())
                };
                splits.push(LayoutSplit {
                    id,
                    direction: *dir,
                    ratio: *ratio,
                    rect: r,
                });
                let (a, b) = match dir {
                    Dir::Right => {
                        let w = (f32::from(r.width) * ratio).round() as u16;
                        (
                            Rect { width: w, ..r },
                            Rect {
                                x: r.x + w,
                                width: r.width - w,
                                ..r
                            },
                        )
                    }
                    Dir::Down => {
                        let h = (f32::from(r.height) * ratio).round() as u16;
                        (
                            Rect { height: h, ..r },
                            Rect {
                                y: r.y + h,
                                height: r.height - h,
                                ..r
                            },
                        )
                    }
                };
                path.push('0');
                first.rects(a, path, panes, splits);
                path.pop();
                path.push('1');
                second.rects(b, path, panes, splits);
                path.pop();
            }
        }
    }
}

impl FakeHerdr {
    pub fn new() -> Self {
        FakeHerdr {
            tabs: Vec::new(),
            panes: Vec::new(),
            agents: Vec::new(),
            focused: None,
            calls: Vec::new(),
            labels: HashMap::new(),
            area: (120, 40),
            texts: HashMap::new(),
            argv: HashMap::new(),
            plugin: Some(true),
            fail: HashMap::new(),
            workspaces: Vec::new(),
            next_pane: 0,
        }
    }

    fn new_pane(&mut self, ws: &str, tab: &str, label: Option<String>) -> String {
        self.next_pane += 1;
        let id = format!("{ws}:p{}", self.next_pane);
        self.panes.push(PaneInfo {
            pane_id: id.clone(),
            tab_id: tab.into(),
            workspace_id: ws.into(),
            label,
            ..PaneInfo::default()
        });
        id
    }

    /// Add a tab with one root pane to the workspace labelled `ws_label`
    /// (created on first use). Ids look like herdr's: tabs are numbered per
    /// workspace (`w2:t1`), panes server-wide (`w2:p7`). The first pane ever
    /// added takes focus.
    pub fn add_tab(&mut self, ws_label: &str, tab_label: &str) -> String {
        let wi = match self.workspaces.iter().position(|(l, _)| l == ws_label) {
            Some(i) => i,
            None => {
                self.workspaces.push((ws_label.into(), 0));
                self.workspaces.len() - 1
            }
        };
        self.workspaces[wi].1 += 1;
        let number = self.workspaces[wi].1;
        let ws = format!("w{}", wi + 1);
        let tab_id = format!("{ws}:t{number}");
        let root = self.new_pane(&ws, &tab_id, None);
        self.argv.insert(root.clone(), vec!["bash".into()]);
        self.focused.get_or_insert(root.clone());
        self.tabs.push(FakeTab {
            info: TabInfo {
                tab_id: tab_id.clone(),
                workspace_id: ws,
                number,
                label: tab_label.into(),
                ..TabInfo::default()
            },
            root: Node::Pane(root),
        });
        tab_id
    }

    /// Close a pane as the user would; its sibling takes the split's place
    /// (and focus, if the closed pane had it), and a tab whose last pane
    /// closes goes away.
    pub fn close_pane(&mut self, pane_id: &str) {
        let Some(ti) = self.tabs.iter().position(|t| t.root.contains(pane_id)) else {
            return;
        };
        let sibling = self.tabs[ti].root.sibling_leaf(pane_id).map(str::to_string);
        if self.focused.as_deref() == Some(pane_id) {
            self.focused = sibling;
        }
        let tab = self.tabs.remove(ti);
        if let Some(root) = tab.root.remove(pane_id) {
            self.tabs.insert(ti, FakeTab { root, ..tab });
        }
        self.panes.retain(|p| p.pane_id != pane_id);
    }

    /// Log a call, failing it if it is in `fail`.
    fn log(&mut self, call: String) -> Result<()> {
        let failure = self.fail.get(&call).copied();
        self.calls.push(call);
        failure.map_or(Ok(()), |f| Err(f()))
    }

    pub fn tree(&self, tab: &str) -> Option<&Node> {
        self.tab(tab).ok().map(|t| &t.root)
    }

    fn tab(&self, tab: &str) -> Result<&FakeTab> {
        self.tabs
            .iter()
            .find(|t| t.info.tab_id == tab)
            .ok_or_else(|| remote("tab_not_found", format!("tab {tab} not found")))
    }

    fn pane_info(&self, id: &str) -> Option<PaneInfo> {
        let p = self.panes.iter().find(|p| p.pane_id == id)?;
        Some(PaneInfo {
            focused: self.focused.as_deref() == Some(id),
            ..p.clone()
        })
    }

    /// The tab's own focused pane: the server-wide one if it's here, else
    /// the first pane.
    fn tab_focus<'a>(&'a self, t: &'a FakeTab) -> &'a str {
        match &self.focused {
            Some(f) if t.root.contains(f) => f,
            _ => t.root.first_leaf(),
        }
    }
}

impl HerdrApi for FakeHerdr {
    fn tabs(&mut self) -> Result<Vec<TabInfo>> {
        self.log("tabs".into())?;
        Ok(self
            .tabs
            .iter()
            .map(|t| {
                let mut leaves = Vec::new();
                t.root.leaves(&mut leaves);
                TabInfo {
                    pane_count: leaves.len() as u32,
                    focused: self.focused.as_deref().is_some_and(|f| leaves.contains(&f)),
                    ..t.info.clone()
                }
            })
            .collect())
    }

    /// In layout order, as herdr lists them.
    fn panes(&mut self, tab: &str) -> Result<Vec<PaneInfo>> {
        self.log(format!("panes {tab}"))?;
        let Ok(t) = self.tab(tab) else {
            return Ok(Vec::new()); // like the real server's filtered pane.list
        };
        let mut leaves = Vec::new();
        t.root.leaves(&mut leaves);
        Ok(leaves
            .into_iter()
            .filter_map(|id| self.pane_info(id))
            .collect())
    }

    fn layout(&mut self, tab: &str) -> Result<LayoutSnapshot> {
        self.log(format!("layout {tab}"))?;
        let t = self.tab(tab)?;
        let area = Rect {
            x: 0,
            y: 0,
            width: self.area.0,
            height: self.area.1,
        };
        let (mut rects, mut splits) = (Vec::new(), Vec::new());
        t.root
            .rects(area, &mut String::new(), &mut rects, &mut splits);
        let focus = self.tab_focus(t);
        Ok(LayoutSnapshot {
            workspace_id: t.info.workspace_id.clone(),
            tab_id: t.info.tab_id.clone(),
            zoomed: false,
            area,
            focused_pane_id: focus.into(),
            panes: rects
                .into_iter()
                .map(|(id, rect)| LayoutPane {
                    focused: id == focus,
                    pane_id: id,
                    rect,
                })
                .collect(),
            splits,
        })
    }

    fn agents(&mut self) -> Result<Vec<AgentInfo>> {
        self.log("agents".into())?;
        Ok(self.agents.clone())
    }

    fn open_plugin_pane(&mut self, req: OpenPane) -> Result<String> {
        let env: Vec<String> = req.env.iter().map(|(k, v)| format!("{k}={v}")).collect();
        let dir = match req.direction {
            Dir::Right => "right",
            Dir::Down => "down",
        };
        self.log(format!(
            "open_plugin_pane {} {} {dir} {}",
            req.entrypoint,
            req.target_pane_id,
            env.join(",")
        ))?;
        let target = req.target_pane_id;
        let Some(ti) = self.tabs.iter().position(|t| t.root.contains(&target)) else {
            return Err(remote("pane_not_found", format!("pane {target} not found")));
        };
        let (ws, tab) = (
            self.tabs[ti].info.workspace_id.clone(),
            self.tabs[ti].info.tab_id.clone(),
        );
        let label = self
            .labels
            .get(req.entrypoint)
            .cloned()
            .unwrap_or_else(|| req.entrypoint.to_string());
        let new = self.new_pane(&ws, &tab, Some(label));
        let exe = "/plugin/target/release/herdr-witch".to_string();
        self.argv
            .insert(new.clone(), vec![exe, req.entrypoint.to_string()]);
        let leaf = self.tabs[ti]
            .root
            .find_leaf(&target)
            .expect("contains checked");
        *leaf = Node::Split {
            dir: req.direction,
            ratio: 0.5,
            first: Box::new(Node::Pane(target)),
            second: Box::new(Node::Pane(new.clone())),
        };
        Ok(new)
    }

    fn set_split_ratio(&mut self, tab: &str, path: &[bool], ratio: f32) -> Result<()> {
        self.log(format!("set_split_ratio {tab} {path:?} {ratio}"))?;
        self.tab(tab)?;
        let t = self
            .tabs
            .iter_mut()
            .find(|t| t.info.tab_id == tab)
            .expect("tab checked");
        let mut node = &mut t.root;
        for &second in path {
            node = match node {
                Node::Split {
                    first, second: s, ..
                } => {
                    if second {
                        s
                    } else {
                        first
                    }
                }
                Node::Pane(_) => {
                    return Err(remote("split_not_found", "split path not found".into()));
                }
            };
        }
        match node {
            Node::Split { ratio: r, .. } => {
                *r = ratio.clamp(0.1, 0.9); // herdr clamps every split ratio
                Ok(())
            }
            Node::Pane(_) => Err(remote("split_not_found", "split path not found".into())),
        }
    }

    fn focused_pane(&mut self) -> Result<Option<PaneInfo>> {
        self.log("focused_pane".into())?;
        Ok(self.focused.clone().and_then(|f| self.pane_info(&f)))
    }

    fn pane_text(&mut self, pane_id: &str) -> Result<String> {
        self.log(format!("pane_text {pane_id}"))?;
        if self.pane_info(pane_id).is_none() {
            return Err(remote(
                "pane_not_found",
                format!("pane {pane_id} not found"),
            ));
        }
        Ok(self.texts.get(pane_id).cloned().unwrap_or_default())
    }

    fn pane_argv(&mut self, pane_id: &str) -> Result<Vec<String>> {
        self.log(format!("pane_argv {pane_id}"))?;
        if self.pane_info(pane_id).is_none() {
            return Err(remote(
                "pane_not_found",
                format!("pane {pane_id} not found"),
            ));
        }
        Ok(self.argv.get(pane_id).cloned().unwrap_or_default())
    }

    fn plugin_enabled(&mut self) -> Result<bool> {
        self.log("plugin_enabled".into())?;
        Ok(self.plugin == Some(true))
    }

    fn remove_pane(&mut self, pane_id: &str) -> Result<()> {
        self.log(format!("remove_pane {pane_id}"))?;
        if self.pane_info(pane_id).is_none() {
            return Err(remote(
                "pane_not_found",
                format!("pane {pane_id} not found"),
            ));
        }
        self.close_pane(pane_id);
        self.argv.remove(pane_id);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn open(entrypoint: &'static str, target: &str, direction: Dir) -> OpenPane {
        OpenPane {
            entrypoint,
            target_pane_id: target.into(),
            direction,
            env: vec![("HERDR_WITCH_LINE".into(), "3".into())],
        }
    }

    fn pane(id: &str) -> Box<Node> {
        Box::new(Node::Pane(id.into()))
    }

    #[test]
    fn fake_records_calls() {
        let mut h = FakeHerdr::new();
        let t = h.add_tab("ws", "tab");
        let root = h.panes(&t).unwrap()[0].pane_id.clone();
        let api: &mut dyn HerdrApi = &mut h; // object-safe
        api.tabs().unwrap();
        h.layout(&t).unwrap();
        h.agents().unwrap();
        h.focused_pane().unwrap();
        let g = h
            .open_plugin_pane(open("grimoire", &root, Dir::Down))
            .unwrap();
        h.set_split_ratio(&t, &[], 0.9).unwrap();
        assert_eq!(
            h.calls,
            [
                format!("panes {t}"),
                "tabs".into(),
                format!("layout {t}"),
                "agents".into(),
                "focused_pane".into(),
                format!("open_plugin_pane grimoire {root} down HERDR_WITCH_LINE=3"),
                format!("set_split_ratio {t} [] 0.9"),
            ]
        );
        assert_ne!(g, root);
    }

    #[test]
    fn fake_argv_and_remove() {
        let mut h = FakeHerdr::new();
        let t = h.add_tab("ws", "tab");
        let root = h.panes(&t).unwrap()[0].pane_id.clone();
        let s = h
            .open_plugin_pane(open("sigil", &root, Dir::Right))
            .unwrap();
        assert_eq!(h.pane_argv(&root).unwrap(), ["bash"]);
        let argv = h.pane_argv(&s).unwrap();
        assert!(
            argv[0].ends_with("/herdr-witch") && argv[1] == "sigil",
            "{argv:?}"
        );
        h.remove_pane(&s).unwrap();
        assert_eq!(h.tree(&t), Some(&Node::Pane(root.clone())));
        assert!(h.remove_pane(&s).is_err(), "already gone");
        assert!(h.pane_argv(&s).is_err());
        assert!(h.calls.contains(&format!("remove_pane {s}")));
    }

    #[test]
    fn fake_ids_are_neutral_and_distinct() {
        let mut h = FakeHerdr::new();
        let a = h.add_tab("one", "a");
        let b = h.add_tab("one", "b");
        let c = h.add_tab("two", "c");
        assert_eq!(
            [a.as_str(), b.as_str(), c.as_str()],
            ["w1:t1", "w1:t2", "w2:t1"]
        );
        let tabs = h.tabs().unwrap();
        assert_eq!(tabs[2].workspace_id, "w2");
        assert_eq!(tabs[1].label, "b");
        let p = h.panes(&c).unwrap();
        assert_eq!(p.len(), 1);
        assert_eq!(p[0].tab_id, c);
        assert_eq!(p[0].workspace_id, "w2");
    }

    #[test]
    fn fake_open_splits_target_and_never_focuses() {
        let mut h = FakeHerdr::new();
        let t = h.add_tab("ws", "tab");
        let root = h.panes(&t).unwrap()[0].pane_id.clone();
        let g = h
            .open_plugin_pane(open("grimoire", &root, Dir::Down))
            .unwrap();
        let s = h
            .open_plugin_pane(open("sigil", &root, Dir::Right))
            .unwrap();
        assert_eq!(
            h.tree(&t).unwrap(),
            &Node::Split {
                dir: Dir::Down,
                ratio: 0.5,
                first: Box::new(Node::Split {
                    dir: Dir::Right,
                    ratio: 0.5,
                    first: pane(&root),
                    second: pane(&s),
                }),
                second: pane(&g),
            }
        );
        assert_eq!(h.focused_pane().unwrap().unwrap().pane_id, root);
        let ps = h.panes(&t).unwrap();
        assert_eq!(ps.len(), 3);
        assert_eq!(h.tabs().unwrap()[0].pane_count, 3);
        let gi = ps.iter().find(|p| p.pane_id == g).unwrap();
        assert!(!gi.focused);
        assert_eq!(gi.label.as_deref(), Some("grimoire"));
        h.labels.insert("sigil", "title".into());
        let s2 = h.open_plugin_pane(open("sigil", &g, Dir::Right)).unwrap();
        let si = h
            .panes(&t)
            .unwrap()
            .into_iter()
            .find(|p| p.pane_id == s2)
            .unwrap();
        assert_eq!(si.label.as_deref(), Some("title"));
        assert!(matches!(
            h.open_plugin_pane(open("sigil", "w9:p9", Dir::Right)),
            Err(HerdrError::Remote { .. })
        ));
    }

    #[test]
    fn fake_unknown_tab_has_no_panes() {
        let mut h = FakeHerdr::new();
        assert_eq!(h.panes("w9:t9").unwrap(), []);
        let t = h.add_tab("ws", "tab");
        let root = h.panes(&t).unwrap()[0].pane_id.clone();
        h.close_pane(&root);
        assert_eq!(h.panes(&t).unwrap(), []);
    }

    #[test]
    fn fake_set_split_ratio_by_path() {
        let mut h = FakeHerdr::new();
        let t = h.add_tab("ws", "tab");
        let root = h.panes(&t).unwrap()[0].pane_id.clone();
        assert!(h.set_split_ratio(&t, &[], 0.5).is_err());
        h.open_plugin_pane(open("grimoire", &root, Dir::Down))
            .unwrap();
        h.open_plugin_pane(open("sigil", &root, Dir::Right))
            .unwrap();
        h.set_split_ratio(&t, &[], 0.88).unwrap();
        h.set_split_ratio(&t, &[false], 0.85).unwrap();
        assert!(matches!(
            h.set_split_ratio(&t, &[true], 0.5),
            Err(HerdrError::Remote { .. })
        ));
        let Node::Split { ratio, first, .. } = h.tree(&t).unwrap() else {
            panic!()
        };
        assert_eq!(*ratio, 0.88);
        let Node::Split { ratio, .. } = first.as_ref() else {
            panic!()
        };
        assert_eq!(*ratio, 0.85);
    }

    #[test]
    fn fake_ratio_clamped_like_herdr() {
        let mut h = FakeHerdr::new();
        let t = h.add_tab("ws", "tab");
        let root = h.panes(&t).unwrap()[0].pane_id.clone();
        h.open_plugin_pane(open("grimoire", &root, Dir::Down))
            .unwrap();
        h.set_split_ratio(&t, &[], 0.95).unwrap();
        let Node::Split { ratio, .. } = h.tree(&t).unwrap() else {
            panic!()
        };
        assert_eq!(*ratio, 0.9);
        h.set_split_ratio(&t, &[], 0.01).unwrap();
        let Node::Split { ratio, .. } = h.tree(&t).unwrap() else {
            panic!()
        };
        assert_eq!(*ratio, 0.1);
    }

    #[test]
    fn fake_plugin_enabled() {
        let mut h = FakeHerdr::new();
        assert!(h.plugin_enabled().unwrap(), "installed and enabled");
        h.plugin = Some(false);
        assert!(!h.plugin_enabled().unwrap(), "disabled");
        h.plugin = None;
        assert!(!h.plugin_enabled().unwrap(), "unlinked");
        assert_eq!(h.calls.last().unwrap(), "plugin_enabled");
    }

    #[test]
    fn fake_injected_failure() {
        let mut h = FakeHerdr::new();
        h.fail.insert("tabs".into(), || {
            HerdrError::Io(std::io::ErrorKind::BrokenPipe.into())
        });
        assert!(matches!(h.tabs(), Err(HerdrError::Io(_))));
        assert_eq!(h.calls, ["tabs"]);
        assert!(h.agents().is_ok());
    }

    #[test]
    fn fake_pane_text() {
        let mut h = FakeHerdr::new();
        let t = h.add_tab("ws", "tab");
        let root = h.panes(&t).unwrap()[0].pane_id.clone();
        assert_eq!(h.pane_text(&root).unwrap(), "");
        h.texts.insert(root.clone(), "$ ls".into());
        assert_eq!(h.pane_text(&root).unwrap(), "$ ls");
        assert_eq!(h.calls.last().unwrap(), &format!("pane_text {root}"));
        assert!(h.pane_text("w9:p9").is_err());
    }

    #[test]
    fn fake_layout_rects() {
        let mut h = FakeHerdr::new();
        let t = h.add_tab("ws", "tab");
        let root = h.panes(&t).unwrap()[0].pane_id.clone();
        let g = h
            .open_plugin_pane(open("grimoire", &root, Dir::Down))
            .unwrap();
        let s = h
            .open_plugin_pane(open("sigil", &root, Dir::Right))
            .unwrap();
        h.set_split_ratio(&t, &[], 0.7).unwrap();
        let l = h.layout(&t).unwrap();
        assert_eq!((l.area.width, l.area.height), (120, 40));
        let rect = |id: &str| l.panes.iter().find(|p| p.pane_id == id).unwrap().rect;
        assert_eq!((rect(&root).height, rect(&root).width), (28, 60));
        assert_eq!((rect(&s).x, rect(&s).width), (60, 60));
        assert_eq!((rect(&g).y, rect(&g).height), (28, 12));
        let ids: Vec<&str> = l.splits.iter().map(|s| s.id.as_str()).collect();
        assert_eq!(ids, ["split_0_root", "split_1_0"]);
        assert_eq!(l.focused_pane_id, root);
    }

    #[test]
    fn fake_close_pane_collapses_split() {
        let mut h = FakeHerdr::new();
        let t = h.add_tab("ws", "tab");
        let root = h.panes(&t).unwrap()[0].pane_id.clone();
        let g = h
            .open_plugin_pane(open("grimoire", &root, Dir::Down))
            .unwrap();
        let s = h
            .open_plugin_pane(open("sigil", &root, Dir::Right))
            .unwrap();
        h.close_pane(&s);
        assert_eq!(
            h.tree(&t).unwrap(),
            &Node::Split {
                dir: Dir::Down,
                ratio: 0.5,
                first: pane(&root),
                second: pane(&g),
            }
        );
        assert_eq!(h.panes(&t).unwrap().len(), 2);
        h.close_pane(&root);
        assert_eq!(h.tree(&t).unwrap(), &Node::Pane(g.clone()));
        assert_eq!(h.focused.as_deref(), Some(g.as_str()));
        h.close_pane(&g);
        assert!(h.tabs().unwrap().is_empty());
        assert!(h.tree(&t).is_none());
    }
}
