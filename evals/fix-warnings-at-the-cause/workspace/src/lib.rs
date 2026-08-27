use std::collections::HashMap;
use std::collections::HashSet;

#[must_use]
pub fn total(values: &[u32]) -> u32 {
    let unused_scratch = values.len();
    let mut seen: HashSet<u32> = HashSet::new();
    let mut sum = 0;
    for value in values {
        if seen.insert(*value) {
            sum += value;
        }
    }
    sum
}
