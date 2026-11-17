use std::{collections::HashSet, io};

use ratatui::layout::Rect;

use crate::session::{MAX_PANES, TreeNode};

const RATIO_BASIS: u64 = 65_536;
const DEFAULT_RATIO: u32 = 32_768;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) enum Axis {
    Columns,
    Rows,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PaneMinimum {
    pub id: u64,
    pub width: u16,
    pub height: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PaneRect {
    pub id: u64,
    pub area: Rect,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Divider {
    pub id: u64,
    pub axis: Axis,
    /// Resize hit area: column gap or the first subtree's bottom-border row.
    pub area: Rect,
    pub parent: Rect,
    pub first_min: u16,
    pub second_min: u16,
}

#[derive(Debug, Default)]
pub(crate) struct Layout {
    pub panes: Vec<PaneRect>,
    pub dividers: Vec<Divider>,
    pub collapsed: bool,
    scratch_mins: Vec<(u32, u32)>,
    scratch_post: Vec<(usize, bool)>,
    scratch_rects: Vec<(usize, Rect)>,
    scratch_lookup: Vec<(u64, u16, u16)>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Node {
    Leaf {
        pane: u64,
    },
    Split {
        divider: u64,
        axis: Axis,
        ratio: u32,
        first: usize,
        second: usize,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SplitTree {
    nodes: Vec<Option<Node>>,
    free: Vec<usize>,
    root: usize,
    next_divider_id: u64,
}

impl SplitTree {
    pub(crate) fn new(pane: u64) -> Self {
        Self {
            nodes: vec![Some(Node::Leaf { pane })],
            free: Vec::new(),
            root: 0,
            next_divider_id: 1,
        }
    }

    pub(crate) fn session_nodes(&self) -> Vec<TreeNode> {
        let live = self.nodes.len() - self.free.len();
        let mut pending = Vec::with_capacity(live);
        let mut nodes = Vec::with_capacity(live);
        pending.push(self.root);
        let mut index = 0;
        while index < pending.len() {
            match self.nodes[pending[index]]
                .as_ref()
                .expect("live split node")
            {
                Node::Leaf { pane } => nodes.push(TreeNode::Leaf { pane_id: *pane }),
                Node::Split {
                    axis,
                    ratio,
                    first,
                    second,
                    ..
                } => {
                    let first_index = pending.len();
                    pending.push(*first);
                    let second_index = pending.len();
                    pending.push(*second);
                    nodes.push(TreeNode::Split {
                        axis: *axis,
                        ratio: *ratio,
                        first: first_index,
                        second: second_index,
                    });
                }
            }
            index += 1;
        }
        nodes
    }

    pub(crate) fn from_session_nodes(saved: Vec<TreeNode>, panes: &[u64]) -> io::Result<Self> {
        let invalid = || io::Error::new(io::ErrorKind::InvalidData, "invalid session split tree");
        if panes.is_empty() || panes.len() > MAX_PANES || saved.len() != panes.len() * 2 - 1 {
            return Err(invalid());
        }
        let mut remaining: HashSet<u64> = panes.iter().copied().collect();
        if remaining.len() != panes.len() || remaining.contains(&0) {
            return Err(invalid());
        }
        let mut parents = vec![0u8; saved.len()];
        let mut nodes = Vec::with_capacity(saved.len());
        let mut next_divider_id = 1;
        for (index, saved) in saved.into_iter().enumerate() {
            let node = match saved {
                TreeNode::Leaf { pane_id } => {
                    if !remaining.remove(&pane_id) {
                        return Err(invalid());
                    }
                    Node::Leaf { pane: pane_id }
                }
                TreeNode::Split {
                    axis,
                    ratio,
                    first,
                    second,
                } => {
                    if ratio > RATIO_BASIS as u32
                        || first <= index
                        || second <= index
                        || first == second
                        || first >= parents.len()
                        || second >= parents.len()
                        || parents[first] != 0
                        || parents[second] != 0
                    {
                        return Err(invalid());
                    }
                    parents[first] = 1;
                    parents[second] = 1;
                    let divider = next_divider_id;
                    next_divider_id += 1;
                    Node::Split {
                        divider,
                        axis,
                        ratio,
                        first,
                        second,
                    }
                }
            };
            nodes.push(Some(node));
        }
        if !remaining.is_empty() || parents[1..].iter().any(|&count| count != 1) {
            return Err(invalid());
        }
        Ok(Self {
            nodes,
            free: Vec::new(),
            root: 0,
            next_divider_id,
        })
    }

    pub(crate) fn split(&mut self, pane: u64, new_pane: u64, axis: Axis) -> Option<u64> {
        if pane == new_pane {
            return None;
        }

        let mut target_idx = None;
        let mut stack = vec![self.root];
        while let Some(idx) = stack.pop() {
            match self.nodes[idx].as_ref()? {
                Node::Leaf { pane: id } => {
                    if *id == new_pane {
                        return None;
                    }
                    if *id == pane {
                        target_idx = Some(idx);
                    }
                }
                Node::Split { first, second, .. } => {
                    stack.push(*second);
                    stack.push(*first);
                }
            }
        }

        let target_idx = target_idx?;
        let divider_id = self.next_divider_id;
        self.next_divider_id = self.next_divider_id.checked_add(1)?;

        let first_idx = self.alloc_node(Node::Leaf { pane });
        let second_idx = self.alloc_node(Node::Leaf { pane: new_pane });
        self.nodes[target_idx] = Some(Node::Split {
            divider: divider_id,
            axis,
            ratio: DEFAULT_RATIO,
            first: first_idx,
            second: second_idx,
        });

        Some(divider_id)
    }

    pub(crate) fn remove(&mut self, pane: u64) -> bool {
        if matches!(self.nodes[self.root], Some(Node::Leaf { .. }) | None) {
            return false;
        }

        let mut stack: Vec<(usize, Option<(usize, bool)>)> = vec![(self.root, None)];
        let mut found = None;
        while let Some((idx, parent)) = stack.pop() {
            let Some(node) = self.nodes[idx].as_ref() else {
                continue;
            };
            match node {
                Node::Leaf { pane: id } => {
                    if *id == pane {
                        found = Some((idx, parent));
                        break;
                    }
                }
                Node::Split { first, second, .. } => {
                    stack.push((*second, Some((idx, false))));
                    stack.push((*first, Some((idx, true))));
                }
            }
        }

        let Some((leaf_idx, Some((parent_idx, is_first)))) = found else {
            return false;
        };

        let sibling_idx = match self.nodes[parent_idx].as_ref() {
            Some(Node::Split { first, second, .. }) => {
                if is_first {
                    *second
                } else {
                    *first
                }
            }
            _ => return false,
        };

        let Some(sibling_node) = self.nodes[sibling_idx].take() else {
            return false;
        };
        self.nodes[parent_idx] = Some(sibling_node);
        self.nodes[leaf_idx] = None;
        self.free.push(leaf_idx);
        self.free.push(sibling_idx);
        true
    }

    pub(crate) fn panes(&self) -> Vec<u64> {
        let mut out = Vec::new();
        let mut stack = vec![self.root];
        while let Some(idx) = stack.pop() {
            let Some(node) = self.nodes[idx].as_ref() else {
                continue;
            };
            match node {
                Node::Leaf { pane } => out.push(*pane),
                Node::Split { first, second, .. } => {
                    stack.push(*second);
                    stack.push(*first);
                }
            }
        }
        out
    }

    pub(crate) fn minimum(&self, mins: &[PaneMinimum]) -> (u32, u32) {
        let mut lookup = Vec::new();
        prepare_lookup(mins, &mut lookup);

        let mut post = vec![(self.root, false)];
        let mut vals: Vec<(u32, u32)> = Vec::new();
        while let Some((idx, visited)) = post.pop() {
            let Some(node) = self.nodes[idx].as_ref() else {
                vals.push((0, 0));
                continue;
            };
            match node {
                Node::Leaf { pane } => {
                    vals.push(lookup_min(mins, &lookup, *pane));
                }
                Node::Split {
                    axis,
                    first,
                    second,
                    ..
                } => {
                    if !visited {
                        post.push((idx, true));
                        post.push((*second, false));
                        post.push((*first, false));
                    } else {
                        let second_min = vals.pop().unwrap_or((0, 0));
                        let first_min = vals.pop().unwrap_or((0, 0));
                        vals.push(combine_min(*axis, first_min, second_min));
                    }
                }
            }
        }
        vals.pop().unwrap_or((0, 0))
    }

    pub(crate) fn can_split(
        &self,
        pane: u64,
        axis: Axis,
        area: Rect,
        mins: &[PaneMinimum],
    ) -> bool {
        let eff = clamp_rect(area);
        if eff.width == 0 || eff.height == 0 {
            return false;
        }

        let mut lookup = Vec::new();
        prepare_lookup(mins, &mut lookup);

        let mut found_pane = false;
        let mut post = vec![(self.root, false)];
        let mut vals: Vec<(u32, u32)> = Vec::new();
        while let Some((idx, visited)) = post.pop() {
            let Some(node) = self.nodes[idx].as_ref() else {
                vals.push((0, 0));
                continue;
            };
            match node {
                Node::Leaf { pane: id } => {
                    let base = lookup_min(mins, &lookup, *id);
                    if *id == pane {
                        found_pane = true;
                        vals.push(combine_min(axis, base, base));
                    } else {
                        vals.push(base);
                    }
                }
                Node::Split {
                    axis: node_axis,
                    first,
                    second,
                    ..
                } => {
                    if !visited {
                        post.push((idx, true));
                        post.push((*second, false));
                        post.push((*first, false));
                    } else {
                        let second_min = vals.pop().unwrap_or((0, 0));
                        let first_min = vals.pop().unwrap_or((0, 0));
                        vals.push(combine_min(*node_axis, first_min, second_min));
                    }
                }
            }
        }

        if !found_pane {
            return false;
        }
        let pred = vals.pop().unwrap_or((0, 0));
        pred.0 <= eff.width as u32 && pred.1 <= eff.height as u32
    }

    pub(crate) fn layout(&self, area: Rect, active: u64, mins: &[PaneMinimum], out: &mut Layout) {
        out.panes.clear();
        out.dividers.clear();
        out.collapsed = false;

        let eff = clamp_rect(area);
        prepare_lookup(mins, &mut out.scratch_lookup);

        out.scratch_mins.clear();
        out.scratch_mins.resize(self.nodes.len(), (0, 0));
        out.scratch_post.clear();
        out.scratch_post.push((self.root, false));

        let mut active_exists = false;
        while let Some((idx, visited)) = out.scratch_post.pop() {
            let Some(node) = self.nodes[idx].as_ref() else {
                continue;
            };
            match node {
                Node::Leaf { pane } => {
                    if *pane == active {
                        active_exists = true;
                    }
                    out.scratch_mins[idx] = lookup_min(mins, &out.scratch_lookup, *pane);
                }
                Node::Split {
                    axis,
                    first,
                    second,
                    ..
                } => {
                    if !visited {
                        out.scratch_post.push((idx, true));
                        out.scratch_post.push((*second, false));
                        out.scratch_post.push((*first, false));
                    } else {
                        let first_min = out.scratch_mins[*first];
                        let second_min = out.scratch_mins[*second];
                        out.scratch_mins[idx] = combine_min(*axis, first_min, second_min);
                    }
                }
            }
        }

        let full_min = out.scratch_mins[self.root];
        if full_min.0 > eff.width as u32 || full_min.1 > eff.height as u32 {
            out.collapsed = true;
            if eff.width > 0 && eff.height > 0 && active_exists {
                out.panes.push(PaneRect {
                    id: active,
                    area: eff,
                });
            }
            return;
        }

        if eff.width == 0 || eff.height == 0 {
            return;
        }

        out.scratch_rects.clear();
        out.scratch_rects.push((self.root, eff));

        while let Some((idx, rect)) = out.scratch_rects.pop() {
            let Some(node) = self.nodes[idx].as_ref() else {
                continue;
            };
            match node {
                Node::Leaf { pane } => {
                    out.panes.push(PaneRect {
                        id: *pane,
                        area: rect,
                    });
                }
                Node::Split {
                    divider,
                    axis,
                    ratio,
                    first,
                    second,
                } => {
                    let (w1, h1) = out.scratch_mins[*first];
                    let (w2, h2) = out.scratch_mins[*second];
                    match *axis {
                        Axis::Columns => {
                            let first_min = w1.min(u16::MAX as u32) as u16;
                            let second_min = w2.min(u16::MAX as u32) as u16;
                            let avail = rect.width - 1;
                            let first_w =
                                split_span(u32::from(avail), *ratio, first_min, second_min);
                            let second_w = avail - first_w;
                            let first_rect = Rect {
                                x: rect.x,
                                y: rect.y,
                                width: first_w,
                                height: rect.height,
                            };
                            let div_rect = Rect {
                                x: rect.x + first_w,
                                y: rect.y,
                                width: 1,
                                height: rect.height,
                            };
                            let second_rect = Rect {
                                x: rect.x + first_w + 1,
                                y: rect.y,
                                width: second_w,
                                height: rect.height,
                            };
                            out.dividers.push(Divider {
                                id: *divider,
                                axis: Axis::Columns,
                                area: div_rect,
                                parent: rect,
                                first_min,
                                second_min,
                            });
                            out.scratch_rects.push((*second, second_rect));
                            out.scratch_rects.push((*first, first_rect));
                        }
                        Axis::Rows => {
                            let first_min = h1.min(u16::MAX as u32) as u16;
                            let second_min = h2.min(u16::MAX as u32) as u16;
                            let avail = u32::from(rect.height);
                            let first_h = split_span(avail, *ratio, first_min, second_min);
                            let second_h = (avail - u32::from(first_h)) as u16;
                            let first_rect = Rect {
                                x: rect.x,
                                y: rect.y,
                                width: rect.width,
                                height: first_h,
                            };
                            let div_rect = Rect {
                                x: rect.x,
                                y: rect.y + first_h.saturating_sub(1),
                                width: rect.width,
                                height: u16::from(first_h > 0),
                            };
                            let second_rect = Rect {
                                x: rect.x,
                                y: rect.y + first_h,
                                width: rect.width,
                                height: second_h,
                            };
                            out.dividers.push(Divider {
                                id: *divider,
                                axis: Axis::Rows,
                                area: div_rect,
                                parent: rect,
                                first_min,
                                second_min,
                            });
                            out.scratch_rects.push((*second, second_rect));
                            out.scratch_rects.push((*first, first_rect));
                        }
                    }
                }
            }
        }
    }

    pub(crate) fn resize(&mut self, divider: &Divider, coordinate: u16) -> bool {
        let eff_parent = clamp_rect(divider.parent);
        if eff_parent.width == 0 || eff_parent.height == 0 {
            return false;
        }

        let (parent_start, total_span, avail, hit_offset) = match divider.axis {
            Axis::Columns => (
                eff_parent.x,
                eff_parent.width,
                u32::from(eff_parent.width) - 1,
                0,
            ),
            Axis::Rows => (
                eff_parent.y,
                eff_parent.height,
                u32::from(eff_parent.height),
                1,
            ),
        };
        let combined = u32::from(divider.first_min) + u32::from(divider.second_min);
        let required = match divider.axis {
            Axis::Columns => combined + 1,
            Axis::Rows => combined,
        };
        if required > total_span as u32 {
            return false;
        }

        let mut target_idx = None;
        let mut stack = vec![self.root];
        while let Some(idx) = stack.pop() {
            let Some(node) = self.nodes[idx].as_ref() else {
                continue;
            };
            match node {
                Node::Leaf { .. } => {}
                Node::Split {
                    divider: id,
                    axis,
                    first,
                    second,
                    ..
                } => {
                    if *id == divider.id {
                        if *axis != divider.axis {
                            return false;
                        }
                        target_idx = Some(idx);
                        break;
                    }
                    stack.push(*second);
                    stack.push(*first);
                }
            }
        }

        let Some(target_idx) = target_idx else {
            return false;
        };

        if avail > 0 {
            let desired = u32::from(coordinate)
                .saturating_add(hit_offset)
                .saturating_sub(u32::from(parent_start));
            let max_first = avail - u32::from(divider.second_min);
            let clamped = desired.clamp(u32::from(divider.first_min), max_first);
            let new_ratio =
                ((clamped as u64 * RATIO_BASIS + (avail as u64 / 2)) / avail as u64) as u32;
            if let Some(Node::Split { ratio, .. }) = self.nodes[target_idx].as_mut() {
                *ratio = new_ratio;
            }
        }

        true
    }

    fn alloc_node(&mut self, node: Node) -> usize {
        if let Some(idx) = self.free.pop() {
            self.nodes[idx] = Some(node);
            idx
        } else {
            let idx = self.nodes.len();
            self.nodes.push(Some(node));
            idx
        }
    }
}

fn clamp_rect(area: Rect) -> Rect {
    Rect {
        x: area.x,
        y: area.y,
        width: area.width.min(u16::MAX - area.x),
        height: area.height.min(u16::MAX - area.y),
    }
}

fn combine_min(axis: Axis, first: (u32, u32), second: (u32, u32)) -> (u32, u32) {
    match axis {
        Axis::Columns => (
            first.0.saturating_add(1).saturating_add(second.0),
            first.1.max(second.1),
        ),
        Axis::Rows => (first.0.max(second.0), first.1.saturating_add(second.1)),
    }
}

fn split_span(avail: u32, ratio: u32, first_min: u16, second_min: u16) -> u16 {
    let raw = ((u64::from(avail) * u64::from(ratio) + RATIO_BASIS / 2) / RATIO_BASIS) as u32;
    let max_first = avail - u32::from(second_min);
    raw.clamp(u32::from(first_min), max_first) as u16
}

fn prepare_lookup(mins: &[PaneMinimum], scratch: &mut Vec<(u64, u16, u16)>) {
    scratch.clear();
    if mins.len() > 16 {
        scratch.extend(mins.iter().map(|m| (m.id, m.width, m.height)));
        scratch.sort_by_key(|m| m.0);
    }
}

fn lookup_min(mins: &[PaneMinimum], scratch: &[(u64, u16, u16)], id: u64) -> (u32, u32) {
    if mins.len() <= 16 {
        for m in mins {
            if m.id == id {
                return (m.width as u32, m.height as u32);
            }
        }
        (0, 0)
    } else {
        let pos = scratch.partition_point(|m| m.0 < id);
        match scratch.get(pos) {
            Some(&(found_id, w, h)) if found_id == id => (w as u32, h as u32),
            _ => (0, 0),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rects_overlap(a: Rect, b: Rect) -> bool {
        a.x < b.right() && b.x < a.right() && a.y < b.bottom() && b.y < a.bottom()
    }

    fn rect_inside(inner: Rect, outer: Rect) -> bool {
        inner.x >= outer.x
            && inner.y >= outer.y
            && inner.right() <= outer.right()
            && inner.bottom() <= outer.bottom()
    }

    fn assert_tiling_invariants(
        tree: &SplitTree,
        area: Rect,
        mins: &[PaneMinimum],
        layout: &Layout,
    ) {
        assert!(!layout.collapsed);
        let expected_panes = tree.panes();
        let got_panes: Vec<u64> = layout.panes.iter().map(|p| p.id).collect();
        assert_eq!(got_panes, expected_panes);
        assert_eq!(layout.dividers.len(), expected_panes.len() - 1);

        let mut all_rects = Vec::with_capacity(layout.panes.len() + layout.dividers.len());
        let mut total_cells: u64 = 0;

        for p in &layout.panes {
            assert!(rect_inside(p.area, area), "pane {:?} outside {:?}", p, area);
            if let Some(min) = mins.iter().find(|m| m.id == p.id) {
                assert!(p.area.width >= min.width);
                assert!(p.area.height >= min.height);
            }
            total_cells += p.area.width as u64 * p.area.height as u64;
            all_rects.push(p.area);
        }

        for d in &layout.dividers {
            assert!(rect_inside(d.area, d.parent));
            assert!(rect_inside(d.parent, area));
            match d.axis {
                Axis::Columns => {
                    assert_eq!(d.area.width, 1);
                    assert_eq!(d.area.height, d.parent.height);
                    total_cells += d.area.width as u64 * d.area.height as u64;
                    all_rects.push(d.area);
                }
                Axis::Rows => {
                    assert_eq!(d.area.height, 1);
                    assert_eq!(d.area.width, d.parent.width);
                    for pane in &layout.panes {
                        if rects_overlap(d.area, pane.area) {
                            assert_eq!(d.area.bottom(), pane.area.bottom());
                        }
                    }
                }
            }
        }

        for i in 0..all_rects.len() {
            for j in (i + 1)..all_rects.len() {
                assert!(
                    !rects_overlap(all_rects[i], all_rects[j]),
                    "rects {:?} and {:?} overlap",
                    all_rects[i],
                    all_rects[j]
                );
            }
        }

        let eff = clamp_rect(area);
        assert_eq!(total_cells, eff.width as u64 * eff.height as u64);
    }

    #[test]
    fn root_leaf_layout_and_remove_refusal() {
        let mut tree = SplitTree::new(10);
        assert_eq!(tree.panes(), vec![10]);
        assert!(!tree.remove(10));
        assert!(!tree.remove(99));

        let mins = [PaneMinimum {
            id: 10,
            width: 12,
            height: 3,
        }];
        assert_eq!(tree.minimum(&mins), (12, 3));

        let area = Rect::new(2, 4, 40, 20);
        let mut layout = Layout::default();
        tree.layout(area, 10, &mins, &mut layout);
        assert!(!layout.collapsed);
        assert!(layout.dividers.is_empty());
        assert_eq!(layout.panes, vec![PaneRect { id: 10, area }]);
    }

    #[test]
    fn split_atomicity_and_monotonic_divider_ids() {
        let mut tree = SplitTree::new(1);
        assert_eq!(tree.split(99, 2, Axis::Columns), None);
        assert_eq!(tree.split(1, 1, Axis::Columns), None);
        assert_eq!(tree.panes(), vec![1]);

        let d1 = tree.split(1, 2, Axis::Columns).expect("valid split");
        assert_eq!(tree.split(1, 2, Axis::Rows), None);
        assert_eq!(tree.split(2, 1, Axis::Rows), None);
        assert_eq!(tree.panes(), vec![1, 2]);

        let d2 = tree.split(2, 3, Axis::Rows).expect("valid split");
        assert!(d2 > d1);
        assert_eq!(tree.panes(), vec![1, 2, 3]);

        assert!(tree.remove(2));
        let d3 = tree.split(3, 4, Axis::Columns).expect("valid split");
        assert!(d3 > d2);
        assert_ne!(d3, d1);
        assert_ne!(d3, d2);
        assert_eq!(tree.panes(), vec![1, 3, 4]);
    }

    #[test]
    fn nested_orientations_and_tiling_invariants() {
        let mut tree = SplitTree::new(1);
        let d_root = tree.split(1, 2, Axis::Columns).unwrap();
        let d_left = tree.split(1, 3, Axis::Rows).unwrap();
        let d_right = tree.split(2, 4, Axis::Rows).unwrap();
        let d_deep = tree.split(4, 5, Axis::Columns).unwrap();
        assert_eq!(tree.panes(), vec![1, 3, 2, 4, 5]);

        let mins = [
            PaneMinimum {
                id: 1,
                width: 10,
                height: 2,
            },
            PaneMinimum {
                id: 2,
                width: 12,
                height: 3,
            },
            PaneMinimum {
                id: 3,
                width: 14,
                height: 4,
            },
            PaneMinimum {
                id: 4,
                width: 10,
                height: 2,
            },
            PaneMinimum {
                id: 5,
                width: 11,
                height: 5,
            },
        ];

        // Left subtree (1 Rows 3): min = (max(10,14), 2+4) = (14, 6)
        // Deep subtree (4 Cols 5): min = (10+1+11, max(2,5)) = (22, 5)
        // Right subtree (2 Rows deep): min = (max(12,22), 3+5) = (22, 8)
        // Root (left Cols right): min = (14+1+22, max(6,8)) = (37, 8)
        assert_eq!(tree.minimum(&mins), (37, 8));

        let area = Rect::new(3, 2, 80, 24);
        let mut layout = Layout::default();
        tree.layout(area, 5, &mins, &mut layout);
        assert_tiling_invariants(&tree, area, &mins, &layout);

        let root_div = layout.dividers.iter().find(|d| d.id == d_root).unwrap();
        assert_eq!(root_div.first_min, 14);
        assert_eq!(root_div.second_min, 22);
        assert_eq!(root_div.parent, area);

        let left_div = layout.dividers.iter().find(|d| d.id == d_left).unwrap();
        assert_eq!(left_div.first_min, 2);
        assert_eq!(left_div.second_min, 4);

        let right_div = layout.dividers.iter().find(|d| d.id == d_right).unwrap();
        assert_eq!(right_div.first_min, 3);
        assert_eq!(right_div.second_min, 5);

        let deep_div = layout.dividers.iter().find(|d| d.id == d_deep).unwrap();
        assert_eq!(deep_div.first_min, 10);
        assert_eq!(deep_div.second_min, 11);
    }

    #[test]
    fn remove_root_child_collapses_sibling_preserving_subtree_and_ratios() {
        let mut tree = SplitTree::new(1);
        let _d_root = tree.split(1, 2, Axis::Columns).unwrap();
        let d_sub = tree.split(2, 3, Axis::Rows).unwrap();

        let mins = [
            PaneMinimum {
                id: 1,
                width: 10,
                height: 2,
            },
            PaneMinimum {
                id: 2,
                width: 10,
                height: 2,
            },
            PaneMinimum {
                id: 3,
                width: 10,
                height: 2,
            },
        ];

        let area = Rect::new(0, 0, 80, 30);
        let mut layout = Layout::default();
        tree.layout(area, 2, &mins, &mut layout);
        let sub_div = *layout.dividers.iter().find(|d| d.id == d_sub).unwrap();
        assert!(tree.resize(&sub_div, 20));

        assert!(tree.remove(1));
        assert_eq!(tree.panes(), vec![2, 3]);

        tree.layout(area, 2, &mins, &mut layout);
        assert_tiling_invariants(&tree, area, &mins, &layout);
        assert_eq!(layout.dividers.len(), 1);
        assert_eq!(layout.dividers[0].id, d_sub);
        assert_eq!(layout.dividers[0].area.y, 20);
    }

    #[test]
    fn resize_clamps_to_descendant_minima_and_restores_ratio_after_shrink() {
        let mut tree = SplitTree::new(1);
        let d_root = tree.split(1, 2, Axis::Columns).unwrap();
        let _d_right = tree.split(2, 3, Axis::Columns).unwrap();

        let mins = [
            PaneMinimum {
                id: 1,
                width: 10,
                height: 2,
            },
            PaneMinimum {
                id: 2,
                width: 12,
                height: 2,
            },
            PaneMinimum {
                id: 3,
                width: 15,
                height: 2,
            },
        ];
        // Right child min width = 12 + 1 + 15 = 28
        let area = Rect::new(0, 0, 100, 20);
        let mut layout = Layout::default();
        tree.layout(area, 1, &mins, &mut layout);

        let root_div = *layout.dividers.iter().find(|d| d.id == d_root).unwrap();
        assert_eq!(root_div.first_min, 10);
        assert_eq!(root_div.second_min, 28);

        // Try dragging divider past right subtree minimum: max x = 99 - 28 = 71
        assert!(tree.resize(&root_div, 95));
        tree.layout(area, 1, &mins, &mut layout);
        assert_tiling_invariants(&tree, area, &mins, &layout);
        let moved = layout.dividers.iter().find(|d| d.id == d_root).unwrap();
        assert_eq!(moved.area.x, 71);

        // Set explicit valid ratio near 70/30 (divider at x = 69)
        assert!(tree.resize(&root_div, 69));
        tree.layout(area, 1, &mins, &mut layout);
        assert_eq!(
            layout
                .dividers
                .iter()
                .find(|d| d.id == d_root)
                .unwrap()
                .area
                .x,
            69
        );

        // Shrink terminal so divider is forced left to fit right subtree min (width 50 -> max x = 49 - 28 = 21)
        let narrow = Rect::new(0, 0, 50, 20);
        tree.layout(narrow, 1, &mins, &mut layout);
        assert_tiling_invariants(&tree, narrow, &mins, &layout);
        let clamped_narrow = layout.dividers.iter().find(|d| d.id == d_root).unwrap();
        assert_eq!(clamped_narrow.area.x, 21);

        // Restore original width: stored ratio must NOT have been overwritten
        tree.layout(area, 1, &mins, &mut layout);
        assert_tiling_invariants(&tree, area, &mins, &layout);
        assert_eq!(
            layout
                .dividers
                .iter()
                .find(|d| d.id == d_root)
                .unwrap()
                .area
                .x,
            69
        );
    }

    #[test]
    fn resize_rejects_stale_mismatched_or_impossible_dividers() {
        let mut tree = SplitTree::new(1);
        let _d_id = tree.split(1, 2, Axis::Columns).unwrap();
        let mins = [
            PaneMinimum {
                id: 1,
                width: 10,
                height: 2,
            },
            PaneMinimum {
                id: 2,
                width: 10,
                height: 2,
            },
        ];

        let area = Rect::new(0, 0, 60, 20);
        let mut layout = Layout::default();
        tree.layout(area, 1, &mins, &mut layout);
        let div = layout.dividers[0];
        let orig_x = div.area.x;

        let wrong_axis = Divider {
            axis: Axis::Rows,
            ..div
        };
        assert!(!tree.resize(&wrong_axis, 15));

        let missing_id = Divider { id: 999, ..div };
        assert!(!tree.resize(&missing_id, 15));

        let impossible = Divider {
            parent: Rect::new(0, 0, 15, 20),
            ..div
        };
        assert!(!tree.resize(&impossible, 12));

        tree.layout(area, 1, &mins, &mut layout);
        assert_eq!(layout.dividers[0].area.x, orig_x);

        assert!(tree.remove(2));
        assert!(!tree.resize(&div, 15));

        let mut rows_tree = SplitTree::new(1);
        let _ = rows_tree.split(1, 2, Axis::Rows).unwrap();
        let r_area = Rect::new(2, 5, 30, 20);
        rows_tree.layout(r_area, 1, &mins, &mut layout);
        let r_div = layout.dividers[0];
        assert!(rows_tree.resize(&r_div, 0));
        rows_tree.layout(r_area, 1, &mins, &mut layout);
        assert_eq!(layout.dividers[0].area.y, r_area.y + mins[0].height - 1);
        assert!(rows_tree.resize(&r_div, u16::MAX));
        rows_tree.layout(r_area, 1, &mins, &mut layout);
        assert_eq!(
            layout.dividers[0].area.y,
            r_area.bottom() - mins[1].height - 1
        );
    }

    #[test]
    fn resize_places_every_cell_accurately_even_at_u16_max() {
        let mins = [
            PaneMinimum {
                id: 1,
                width: 1,
                height: 1,
            },
            PaneMinimum {
                id: 2,
                width: 1,
                height: 1,
            },
        ];
        for axis in [Axis::Columns, Axis::Rows] {
            let mut tree = SplitTree::new(1);
            tree.split(1, 2, axis).unwrap();
            let area = match axis {
                Axis::Columns => Rect::new(0, 0, u16::MAX, 10),
                Axis::Rows => Rect::new(0, 0, 10, u16::MAX),
            };
            let mut layout = Layout::default();
            tree.layout(area, 1, &mins, &mut layout);
            let div = layout.dividers[0];
            for target in [0, 1, 2, 17, 16384, 32767, 32768, 65532, 65533, 65534] {
                if target == 65534 || (axis == Axis::Columns && target == 0) {
                    continue;
                }
                assert!(tree.resize(&div, target));
                tree.layout(area, 1, &mins, &mut layout);
                assert_tiling_invariants(&tree, area, &mins, &layout);
                let position = match axis {
                    Axis::Columns => layout.dividers[0].area.x,
                    Axis::Rows => layout.dividers[0].area.y,
                };
                assert_eq!(position, target);
            }
        }
    }

    #[test]
    fn can_split_predicts_recursive_minima_and_refuses_shortfall() {
        let mut tree = SplitTree::new(1);
        tree.split(1, 2, Axis::Columns).unwrap();
        let mins = [
            PaneMinimum {
                id: 1,
                width: 10,
                height: 2,
            },
            PaneMinimum {
                id: 2,
                width: 12,
                height: 2,
            },
        ];

        // Splitting pane 1 along Columns predicts (10+1+10) + 1 + 12 = 34 width, 2 height
        assert!(tree.can_split(1, Axis::Columns, Rect::new(0, 0, 34, 2), &mins));
        assert!(!tree.can_split(1, Axis::Columns, Rect::new(0, 0, 33, 2), &mins));

        // Splitting pane 2 along Rows predicts 23 width and 2+2 = 4 height.
        assert!(tree.can_split(2, Axis::Rows, Rect::new(0, 0, 23, 4), &mins));
        assert!(!tree.can_split(2, Axis::Rows, Rect::new(0, 0, 23, 3), &mins));

        // Missing pane and zero areas
        assert!(!tree.can_split(99, Axis::Columns, Rect::new(0, 0, 100, 100), &mins));
        assert!(!tree.can_split(1, Axis::Columns, Rect::new(0, 0, 0, 10), &[]));
        assert!(!tree.can_split(1, Axis::Columns, Rect::new(0, 0, 10, 0), &[]));
    }

    #[test]
    fn collapse_shows_only_active_including_hidden_pane_and_restores() {
        let mut tree = SplitTree::new(1);
        let d1 = tree.split(1, 2, Axis::Columns).unwrap();
        let _d2 = tree.split(2, 3, Axis::Rows).unwrap();
        let mins = [
            PaneMinimum {
                id: 1,
                width: 10,
                height: 2,
            },
            PaneMinimum {
                id: 2,
                width: 10,
                height: 2,
            },
            PaneMinimum {
                id: 3,
                width: 10,
                height: 2,
            },
        ];
        // Full minimum = (21, 4), retaining both independent frame borders.
        let full = Rect::new(1, 1, 60, 20);
        let mut layout = Layout::default();
        tree.layout(full, 3, &mins, &mut layout);
        let div1 = *layout.dividers.iter().find(|d| d.id == d1).unwrap();
        assert!(tree.resize(&div1, 40));
        tree.layout(full, 3, &mins, &mut layout);
        let saved_x = layout.dividers.iter().find(|d| d.id == d1).unwrap().area.x;

        // Collapse by width, active is nested pane 3
        let too_narrow = Rect::new(1, 1, 20, 20);
        tree.layout(too_narrow, 3, &mins, &mut layout);
        assert!(layout.collapsed);
        assert!(layout.dividers.is_empty());
        assert_eq!(
            layout.panes,
            vec![PaneRect {
                id: 3,
                area: too_narrow
            }]
        );

        // Collapse by height, active is pane 2
        let min_height = u16::try_from(tree.minimum(&mins).1).unwrap();
        let too_short = Rect::new(1, 1, 60, min_height - 1);
        tree.layout(too_short, 2, &mins, &mut layout);
        assert!(layout.collapsed);
        assert!(layout.dividers.is_empty());
        assert_eq!(
            layout.panes,
            vec![PaneRect {
                id: 2,
                area: too_short
            }]
        );

        // Missing active pane never produces phantom pane hit
        tree.layout(too_narrow, 999, &mins, &mut layout);
        assert!(layout.collapsed);
        assert!(layout.panes.is_empty());
        assert!(layout.dividers.is_empty());

        // Restore full room
        tree.layout(full, 3, &mins, &mut layout);
        assert_tiling_invariants(&tree, full, &mins, &layout);
        assert_eq!(
            layout.dividers.iter().find(|d| d.id == d1).unwrap().area.x,
            saved_x
        );
    }

    #[test]
    fn zero_area_and_extreme_bounds_and_subtree_minima_overflow() {
        let mut tree = SplitTree::new(1);
        tree.split(1, 2, Axis::Columns).unwrap();

        // Zero areas never panic and produce no drawable panes or dividers
        let mut layout = Layout::default();
        let mins = [
            PaneMinimum {
                id: 1,
                width: 10,
                height: 2,
            },
            PaneMinimum {
                id: 2,
                width: 10,
                height: 2,
            },
        ];
        for zero in [
            Rect::new(0, 0, 0, 0),
            Rect::new(5, 5, 0, 10),
            Rect::new(5, 5, 10, 0),
        ] {
            tree.layout(zero, 1, &mins, &mut layout);
            assert!(layout.collapsed);
            assert!(layout.panes.is_empty());
            assert!(layout.dividers.is_empty());
        }

        // Subtree minima overflow u16: 40000 + 1 + 40000 = 80001 > u16::MAX (65535)
        let huge_mins = [
            PaneMinimum {
                id: 1,
                width: 40_000,
                height: 10,
            },
            PaneMinimum {
                id: 2,
                width: 40_000,
                height: 10,
            },
        ];
        assert_eq!(tree.minimum(&huge_mins), (80_001, 10));
        let max_area = Rect::new(0, 0, u16::MAX, 20);
        assert!(!tree.can_split(1, Axis::Columns, max_area, &huge_mins));
        tree.layout(max_area, 2, &huge_mins, &mut layout);
        assert!(layout.collapsed);
        assert_eq!(
            layout.panes,
            vec![PaneRect {
                id: 2,
                area: max_area
            }]
        );

        // Extreme offset near u16::MAX
        let edge_area = Rect::new(u16::MAX - 25, u16::MAX - 10, 25, 10);
        tree.layout(edge_area, 1, &mins, &mut layout);
        assert_tiling_invariants(&tree, edge_area, &mins, &layout);

        // Out-of-bounds literal Rect clamps safely without u16 overflow
        let oob_area = Rect {
            x: u16::MAX,
            y: u16::MAX,
            width: 10,
            height: 10,
        };
        tree.layout(oob_area, 1, &mins, &mut layout);
        assert!(layout.collapsed);
        assert!(layout.panes.is_empty());
        assert!(layout.dividers.is_empty());
    }

    #[test]
    fn deep_tree_without_arbitrary_pane_cap_collapses_and_restores() {
        let mut tree = SplitTree::new(0);
        let mut mins = vec![PaneMinimum {
            id: 0,
            width: 1,
            height: 1,
        }];
        for i in 1..=300u64 {
            assert!(tree.split(i - 1, i, Axis::Columns).is_some());
            mins.push(PaneMinimum {
                id: i,
                width: 1,
                height: 1,
            });
        }
        assert_eq!(tree.panes().len(), 301);
        assert_eq!(tree.minimum(&mins), (601, 1));

        let area = Rect::new(0, 0, 1000, 10);
        let mut layout = Layout::default();
        tree.layout(area, 150, &mins, &mut layout);
        assert_tiling_invariants(&tree, area, &mins, &layout);

        // Collapse and restore without losing the stored topology.
        tree.layout(Rect::new(0, 0, 50, 10), 150, &mins, &mut layout);
        assert!(layout.collapsed);
        assert_eq!(layout.panes.len(), 1);
        assert!(layout.dividers.is_empty());

        tree.layout(area, 150, &mins, &mut layout);
        assert_tiling_invariants(&tree, area, &mins, &layout);
    }
}
