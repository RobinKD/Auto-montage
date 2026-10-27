//! `difflib.SequenceMatcher` de Python, à l'identique (mêmes blocs, mêmes opérations, même
//! rapport), pour les corrections de sous-titres (build_edit) et les reprises (moments_lib).

use std::collections::{HashMap, HashSet};
use std::hash::Hash;

pub struct SequenceMatcher<'a, T: Eq + Hash> {
    a: &'a [T],
    b: &'a [T],
    b2j: HashMap<&'a T, Vec<usize>>,
    bjunk: HashSet<&'a T>,
    bpopular: HashSet<&'a T>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tag {
    Replace,
    Delete,
    Insert,
    Equal,
}

#[cfg(test)]
impl Tag {
    pub fn as_str(&self) -> &'static str {
        match self {
            Tag::Replace => "replace",
            Tag::Delete => "delete",
            Tag::Insert => "insert",
            Tag::Equal => "equal",
        }
    }
}

impl Tag {
}

impl<'a, T: Eq + Hash> SequenceMatcher<'a, T> {
    /// Sans fonction « junk », autojunk activé (comme `SequenceMatcher(None, a, b)`).
    pub fn new(a: &'a [T], b: &'a [T]) -> Self {
        let mut b2j: HashMap<&T, Vec<usize>> = HashMap::new();
        for (i, x) in b.iter().enumerate() {
            b2j.entry(x).or_default().push(i);
        }
        let mut bpopular = HashSet::new();
        let n = b.len();
        if n >= 200 {
            let ntest = n / 100 + 1;
            let popular: Vec<&T> = b2j.iter().filter(|(_, v)| v.len() > ntest).map(|(k, _)| *k).collect();
            for k in popular {
                b2j.remove(k);
                bpopular.insert(k);
            }
        }
        SequenceMatcher { a, b, b2j, bjunk: HashSet::new(), bpopular }
    }

    /// (i, j, k) : plus long bloc commun dans a[alo:ahi] et b[blo:bhi].
    pub fn find_longest_match(&self, alo: usize, ahi: usize, blo: usize, bhi: usize) -> (usize, usize, usize) {
        let (a, b) = (self.a, self.b);
        let isbjunk = |x: &T| self.bjunk.contains(x);
        let (mut besti, mut bestj, mut bestsize) = (alo, blo, 0usize);
        let mut j2len: HashMap<usize, usize> = HashMap::new();
        for i in alo..ahi {
            let mut newj2len: HashMap<usize, usize> = HashMap::new();
            if let Some(js) = self.b2j.get(&a[i]) {
                for &j in js {
                    if j < blo {
                        continue;
                    }
                    if j >= bhi {
                        break;
                    }
                    let k = j2len.get(&(j.wrapping_sub(1))).copied().unwrap_or(0) + 1;
                    let k = if j == 0 { 1 } else { k };
                    newj2len.insert(j, k);
                    if k > bestsize {
                        besti = i + 1 - k;
                        bestj = j + 1 - k;
                        bestsize = k;
                    }
                }
            }
            j2len = newj2len;
        }
        while besti > alo && bestj > blo && !isbjunk(&b[bestj - 1]) && a[besti - 1] == b[bestj - 1] {
            besti -= 1;
            bestj -= 1;
            bestsize += 1;
        }
        while besti + bestsize < ahi && bestj + bestsize < bhi && !isbjunk(&b[bestj + bestsize]) && a[besti + bestsize] == b[bestj + bestsize] {
            bestsize += 1;
        }
        while besti > alo && bestj > blo && isbjunk(&b[bestj - 1]) && a[besti - 1] == b[bestj - 1] {
            besti -= 1;
            bestj -= 1;
            bestsize += 1;
        }
        while besti + bestsize < ahi && bestj + bestsize < bhi && isbjunk(&b[bestj + bestsize]) && a[besti + bestsize] == b[bestj + bestsize] {
            bestsize += 1;
        }
        let _ = &self.bpopular;
        (besti, bestj, bestsize)
    }

    pub fn get_matching_blocks(&self) -> Vec<(usize, usize, usize)> {
        let (la, lb) = (self.a.len(), self.b.len());
        let mut queue = vec![(0, la, 0, lb)];
        let mut blocks = vec![];
        while let Some((alo, ahi, blo, bhi)) = queue.pop() {
            let (i, j, k) = self.find_longest_match(alo, ahi, blo, bhi);
            if k > 0 {
                blocks.push((i, j, k));
                if alo < i && blo < j {
                    queue.push((alo, i, blo, j));
                }
                if i + k < ahi && j + k < bhi {
                    queue.push((i + k, ahi, j + k, bhi));
                }
            }
        }
        blocks.sort();
        let (mut i1, mut j1, mut k1) = (0, 0, 0);
        let mut non_adjacent = vec![];
        for (i2, j2, k2) in blocks {
            if i1 + k1 == i2 && j1 + k1 == j2 {
                k1 += k2;
            } else {
                if k1 > 0 {
                    non_adjacent.push((i1, j1, k1));
                }
                i1 = i2;
                j1 = j2;
                k1 = k2;
            }
        }
        if k1 > 0 {
            non_adjacent.push((i1, j1, k1));
        }
        non_adjacent.push((la, lb, 0));
        non_adjacent
    }

    pub fn get_opcodes(&self) -> Vec<(Tag, usize, usize, usize, usize)> {
        let (mut i, mut j) = (0, 0);
        let mut answer = vec![];
        for (ai, bj, size) in self.get_matching_blocks() {
            let tag = if i < ai && j < bj {
                Some(Tag::Replace)
            } else if i < ai {
                Some(Tag::Delete)
            } else if j < bj {
                Some(Tag::Insert)
            } else {
                None
            };
            if let Some(t) = tag {
                answer.push((t, i, ai, j, bj));
            }
            i = ai + size;
            j = bj + size;
            if size > 0 {
                answer.push((Tag::Equal, ai, i, bj, j));
            }
        }
        answer
    }

    pub fn ratio(&self) -> f64 {
        let matches: usize = self.get_matching_blocks().iter().map(|b| b.2).sum();
        let length = self.a.len() + self.b.len();
        if length > 0 {
            2.0 * matches as f64 / length as f64
        } else {
            1.0
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn like_python() {
        // Python : SequenceMatcher(None, "abxcd", "abcd").get_opcodes()
        let a: Vec<char> = "abxcd".chars().collect();
        let b: Vec<char> = "abcd".chars().collect();
        let m = SequenceMatcher::new(&a, &b);
        let ops: Vec<_> = m.get_opcodes().into_iter().map(|o| (o.0.as_str(), o.1, o.2, o.3, o.4)).collect();
        assert_eq!(ops, vec![("equal", 0, 2, 0, 2), ("delete", 2, 3, 2, 2), ("equal", 3, 5, 2, 4)]);
        assert!((m.ratio() - 8.0 / 9.0).abs() < 1e-12);
        let a: Vec<&str> = "le chat dort".split(' ').collect();
        let b: Vec<&str> = "le gros chat dort bien".split(' ').collect();
        let ops: Vec<_> = SequenceMatcher::new(&a, &b).get_opcodes().into_iter().map(|o| (o.0.as_str(), o.1, o.2, o.3, o.4)).collect();
        assert_eq!(ops, vec![("equal", 0, 1, 0, 1), ("insert", 1, 1, 1, 2), ("equal", 1, 3, 2, 4), ("insert", 3, 3, 4, 5)]);
    }
}
