//! Window-local terminal layout. Leaves reference persistent terminal IDs.
use serde::{Deserialize, Serialize};
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Axis {
    Right,
    Down,
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Tree {
    Leaf {
        id: usize,
    },
    Split {
        axis: Axis,
        ratio: u16,
        first: Box<Tree>,
        second: Box<Tree>,
    },
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct Area {
    pub x: u16,
    pub y: u16,
    pub width: u16,
    pub height: u16,
}
impl Area {
    pub fn contains(self, p: [f64; 2]) -> bool {
        p[0] >= f64::from(self.x)
            && p[0] < f64::from(self.x) + f64::from(self.width)
            && p[1] >= f64::from(self.y)
            && p[1] < f64::from(self.y) + f64::from(self.height)
    }
}
#[derive(Clone, Debug)]
pub struct Divider {
    pub path: Vec<bool>,
    pub axis: Axis,
    pub rect: Area,
    pub parent: Area,
    pub leaves: Vec<usize>,
}
#[derive(Default)]
pub struct Layout {
    pub panes: Vec<(usize, Area)>,
    pub dividers: Vec<Divider>,
}
impl Tree {
    pub fn leaf(id: usize) -> Self {
        Self::Leaf { id }
    }
    pub fn pair(first: usize, second: usize, axis: Axis, ratio: u16) -> Self {
        Self::Split {
            axis,
            ratio,
            first: Box::new(Self::leaf(first)),
            second: Box::new(Self::leaf(second)),
        }
    }
    pub fn ids(&self) -> Vec<usize> {
        match self {
            Self::Leaf { id } => vec![*id],
            Self::Split { first, second, .. } => {
                first.ids().into_iter().chain(second.ids()).collect()
            }
        }
    }
    pub fn valid(&self, allowed: &[usize]) -> bool {
        let ids = self.ids();
        if ids.len() > 32 || ids.iter().any(|id| !allowed.contains(id)) {
            return false;
        }
        let mut unique = ids.clone();
        unique.sort_unstable();
        unique.dedup();
        unique.len() == ids.len() && self.ratios_valid()
    }
    fn ratios_valid(&self) -> bool {
        match self {
            Self::Leaf { .. } => true,
            Self::Split {
                ratio,
                first,
                second,
                ..
            } => (1000..=9000).contains(ratio) && first.ratios_valid() && second.ratios_valid(),
        }
    }
    pub fn replace(&mut self, old: usize, new: usize) {
        match self {
            Self::Leaf { id } => {
                if *id == old {
                    *id = new;
                }
            }
            Self::Split { first, second, .. } => {
                first.replace(old, new);
                second.replace(old, new);
            }
        }
    }
    pub fn map_ids<F: Fn(usize) -> usize>(&mut self, map: &F) {
        match self {
            Self::Leaf { id } => *id = map(*id),
            Self::Split { first, second, .. } => {
                first.map_ids(map);
                second.map_ids(map);
            }
        }
    }
    pub fn swap(&mut self, a: usize, b: usize) {
        match self {
            Self::Leaf { id } => {
                if *id == a {
                    *id = b;
                } else if *id == b {
                    *id = a;
                }
            }
            Self::Split { first, second, .. } => {
                first.swap(a, b);
                second.swap(a, b);
            }
        }
    }
    pub fn split(&mut self, target: usize, new: usize, axis: Axis) -> bool {
        match self {
            Self::Leaf { id } if *id == target => {
                *self = Self::pair(target, new, axis, 5000);
                true
            }
            Self::Leaf { .. } => false,
            Self::Split { first, second, .. } => {
                first.split(target, new, axis) || second.split(target, new, axis)
            }
        }
    }
    pub fn retain(self, allowed: &[usize]) -> Option<Self> {
        match self {
            Self::Leaf { id } => allowed.contains(&id).then_some(Self::leaf(id)),
            Self::Split {
                axis,
                ratio,
                first,
                second,
            } => match (first.retain(allowed), second.retain(allowed)) {
                (Some(a), Some(b)) => Some(Self::Split {
                    axis,
                    ratio,
                    first: Box::new(a),
                    second: Box::new(b),
                }),
                (Some(a), None) | (None, Some(a)) => Some(a),
                _ => None,
            },
        }
    }
    pub fn set_ratio(&mut self, path: &[bool], ratio: u16) -> bool {
        match self {
            Self::Split {
                ratio: r,
                first,
                second,
                ..
            } => {
                if let Some((direction, rest)) = path.split_first() {
                    if *direction {
                        second.set_ratio(rest, ratio)
                    } else {
                        first.set_ratio(rest, ratio)
                    }
                } else {
                    *r = ratio.clamp(1000, 9000);
                    true
                }
            }
            _ => false,
        }
    }
    pub fn path_to(&self, target: usize) -> Option<Vec<bool>> {
        match self {
            Self::Leaf { id } => (*id == target).then(Vec::new),
            Self::Split { first, second, .. } => {
                if let Some(mut path) = first.path_to(target) {
                    path.insert(0, false);
                    Some(path)
                } else {
                    second.path_to(target).map(|mut path| {
                        path.insert(0, true);
                        path
                    })
                }
            }
        }
    }
    pub fn ratio_at(&self, path: &[bool]) -> Option<u16> {
        match self {
            Self::Split {
                ratio,
                first,
                second,
                ..
            } => match path.split_first() {
                None => Some(*ratio),
                Some((true, rest)) => second.ratio_at(rest),
                Some((false, rest)) => first.ratio_at(rest),
            },
            _ => None,
        }
    }
    pub fn root_ratio(&self) -> f64 {
        match self {
            Self::Split { ratio, .. } => f64::from(*ratio) / 10000.,
            _ => 0.5,
        }
    }
    fn minimum(&self, gap: u16, leaf: [u16; 2]) -> [u16; 2] {
        match self {
            Self::Leaf { .. } => leaf,
            Self::Split {
                axis,
                first,
                second,
                ..
            } => {
                let a = first.minimum(gap, leaf);
                let b = second.minimum(gap, leaf);
                match axis {
                    Axis::Right => [
                        a[0].saturating_add(b[0]).saturating_add(gap),
                        a[1].max(b[1]),
                    ],
                    Axis::Down => [
                        a[0].max(b[0]),
                        a[1].saturating_add(b[1]).saturating_add(gap),
                    ],
                }
            }
        }
    }
    pub fn layout(&self, area: Area, gap: u16, minimum: [u16; 2], selected: usize) -> Layout {
        let mut result = Layout::default();
        self.place(area, gap, minimum, selected, &mut vec![], &mut result);
        let ids = self.ids();
        for d in &mut result.dividers {
            d.leaves = ids.clone();
        }
        result
    }
    fn place(
        &self,
        area: Area,
        gap: u16,
        minimum: [u16; 2],
        selected: usize,
        path: &mut Vec<bool>,
        out: &mut Layout,
    ) {
        if area.width == 0 || area.height == 0 {
            return;
        }
        if let Self::Split {
            axis,
            ratio,
            first,
            second,
        } = self
        {
            let a = first.minimum(gap, minimum);
            let b = second.minimum(gap, minimum);
            let n = usize::from(*axis == Axis::Down);
            let length = if n == 0 { area.width } else { area.height };
            if length >= a[n].saturating_add(b[n]).saturating_add(gap) {
                let available = length - gap;
                let size = ((u32::from(available) * u32::from(*ratio) / 10000) as u16)
                    .clamp(a[n], available - b[n]);
                let (mut left, mut right, mut divider) = (area, area, area);
                if n == 0 {
                    left.width = size;
                    right.x += size + gap;
                    right.width = available - size;
                    divider.x += size;
                    divider.width = gap;
                } else {
                    left.height = size;
                    right.y += size + gap;
                    right.height = available - size;
                    divider.y += size;
                    divider.height = gap;
                }
                out.dividers.push(Divider {
                    path: path.clone(),
                    axis: *axis,
                    rect: divider,
                    parent: area,
                    leaves: vec![],
                });
                path.push(false);
                first.place(left, gap, minimum, selected, path, out);
                path.pop();
                path.push(true);
                second.place(right, gap, minimum, selected, path, out);
                path.pop();
                return;
            }
        }
        let ids = self.ids();
        out.panes.push((
            if ids.contains(&selected) {
                selected
            } else {
                ids[0]
            },
            area,
        ));
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn nested_geometry_fills_without_overlap() {
        let mut tree = Tree::pair(0, 1, Axis::Right, 5000);
        tree.split(1, 2, Axis::Down);
        tree.split(0, 3, Axis::Down);
        let area = Area {
            x: 5,
            y: 4,
            width: 81,
            height: 25,
        };
        let layout = tree.layout(area, 1, [8, 3], 2);
        assert_eq!(layout.panes.len(), 4);
        assert_eq!(layout.dividers.len(), 3);
        let regions = layout
            .panes
            .iter()
            .map(|(_, r)| r)
            .chain(layout.dividers.iter().map(|d| &d.rect))
            .collect::<Vec<_>>();
        assert_eq!(
            regions
                .iter()
                .map(|r| u32::from(r.width) * u32::from(r.height))
                .sum::<u32>(),
            81 * 25
        );
        for (i, a) in regions.iter().enumerate() {
            assert!(a.x >= 5 && a.y >= 4 && a.x + a.width <= 86 && a.y + a.height <= 29);
            for b in regions.iter().skip(i + 1) {
                assert!(
                    a.x + a.width <= b.x
                        || b.x + b.width <= a.x
                        || a.y + a.height <= b.y
                        || b.y + b.height <= a.y
                );
            }
        }
    }
    #[test]
    fn pruning_swapping_and_small_viewports_keep_identity() {
        let mut tree = Tree::pair(0, 1, Axis::Right, 5000);
        tree.split(1, 2, Axis::Down);
        tree.swap(0, 2);
        assert_eq!(tree.ids(), vec![2, 1, 0]);
        tree.set_ratio(&[true], 7000);
        assert!(tree.valid(&[0, 1, 2]));
        let small = tree.layout(
            Area {
                x: 0,
                y: 0,
                width: 6,
                height: 3,
            },
            1,
            [8, 3],
            1,
        );
        assert_eq!(small.panes.len(), 1);
        assert_eq!(small.panes[0].0, 1);
        let tree = tree.retain(&[0, 2]).unwrap();
        assert_eq!(tree.ids(), vec![2, 0]);
        assert!(tree.valid(&[0, 2]));
        let restored: Tree = serde_json::from_slice(&serde_json::to_vec(&tree).unwrap()).unwrap();
        assert_eq!(tree, restored);
    }
}
