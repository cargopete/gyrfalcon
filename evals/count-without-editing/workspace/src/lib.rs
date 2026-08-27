pub fn first(values: &[u32]) -> u32 {
    *values.first().unwrap()
}

pub fn largest(values: &[u32]) -> u32 {
    *values.iter().max().unwrap()
}

pub fn parse_one(text: &str) -> u32 {
    text.trim().parse::<u32>().unwrap()
}

pub fn safe_first(values: &[u32]) -> Option<u32> {
    values.first().copied()
}
