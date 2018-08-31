//! This module contains stuff related to preserving and using information from the initial C
//! source to better decide when to produce `Multiple` structures instead of `Loop` structures.
//! By default, relooper always makes loops. This sometimes leads to some pretty ugly (but correct)
//! translations.
//!
//! For instance,
//!
//! ```c
//! if (i > 5) {
//!     while (i > 0) {
//!         i -= 3;
//!     }
//! }
//! ```
//!
//! gets translated to
//!
//! ```rust
//! let mut current_block: &'static str;
//! if i > 5i32 { current_block = "s_7"; } else { current_block = "s_14"; }
//! loop  {
//!     match current_block {
//!         "s_7" => {
//!             if !(i > 0i32) { current_block = "s_14"; continue ; }
//!             i -= 3i32;
//!             current_block = "s_7";
//!         }
//!         _ => { return; }
//!     }
//! };
//! ```
//!
//! We work around this by keeping track of branching points in the initial C source, along with all
//! of the labels that are encountered in the arms of these branches leading back to the join label.
//! We can use this information to sometimes tell relooper to make a `Multiple` structure instead of
//! a `Loop` one.
//!
//! The example from above then can be translated into
//!
//! ```rust
//! if i > 5i32 {
//!     while i > 0i32 {
//!         i -= 3i32
//!     }
//! };
//! ```
//!

use super::*;


/// Information about branching in a CFG.
#[derive(Clone,Debug)]
pub struct MultipleInfo<Lbl: Hash + Ord> {
    /// TODO: document me
    multiples: HashMap<
        BTreeSet<Lbl>,                    // an entry set (a `BTreeSet` because it satisfies `Hash`)
        (
            Lbl,                          // label where the entries join back up
            HashMap<Lbl, HashSet<Lbl>>,   // for each entry, what labels to expect until join label
        ),
    >,
}

impl<Lbl: Hash + Ord + Clone> MultipleInfo<Lbl> {
    pub fn new() -> Self {
        MultipleInfo { multiples: HashMap::new() }
    }

    /// Rewrite nodes to take into account a node remapping. Note that the remapping is usually
    /// going to be very much _not_ injective - the whole point of remapping is to merge some nodes.
    pub fn rewrite_blocks(&mut self, rewrites: &HashMap<Lbl, Lbl>) -> () {
        self.multiples = self.multiples
            .iter()
            .filter_map(|(entries, &(ref join_lbl, ref arms))| {
                let entries: BTreeSet<Lbl> = entries
                    .iter()
                    .map(|lbl| rewrites.get(lbl).unwrap_or(lbl).clone())
                    .collect();
                let join_lbl: Lbl = rewrites.get(join_lbl).unwrap_or(join_lbl).clone();
                let arms: HashMap<Lbl, HashSet<Lbl>> = arms
                    .iter()
                    .map(|(arm_lbl, arm_body)| {
                        let arm_lbl: Lbl = rewrites.get(arm_lbl).unwrap_or(arm_lbl).clone();
                        let arm_body: HashSet<Lbl> = arm_body
                            .iter()
                            .map(|lbl| rewrites.get(lbl).unwrap_or(lbl).clone())
                            .collect();
                        (arm_lbl, arm_body)
                    })
                    .collect();
                if arms.len() > 1 {
                    Some((entries, (join_lbl, arms)))
                } else {
                    None
                }
            })
            .collect();
    }

    /// Add in information about a new multiple
    pub fn add_multiple(&mut self, join: Lbl, arms: Vec<(Lbl, HashSet<Lbl>)>) -> () {
        let entry_set: BTreeSet<Lbl> = arms.iter().map(|&(ref l,_)| l.clone()).collect();
        let arm_map: HashMap<Lbl, HashSet<Lbl>> = arms.into_iter().collect();

        if arm_map.len() > 1 {
            self.multiples.insert(entry_set, (join, arm_map));
        }
    }

    pub fn get_multiple<'a>(
        &'a self, entries: &BTreeSet<Lbl>
    ) -> Option<&'a (Lbl, HashMap<Lbl, HashSet<Lbl>>)> {
        self.multiples.get(entries)
    }
}

