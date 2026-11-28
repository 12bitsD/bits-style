use std::ops::Range;

use regex::Regex;

pub(crate) fn inside_sorted_ranges(
    ranges: &[Range<usize>],
    offset: usize,
) -> bool {
    let index = ranges.partition_point(|range| range.start <= offset);
    index > 0 && ranges[..index].iter().any(|range| range.end > offset)
}

pub(crate) fn ranges_overlap(
    ranges: &[Range<usize>],
    start: usize,
    end: usize,
) -> bool {
    let index = ranges.partition_point(|range| range.start < end);
    index > 0 && ranges[..index].iter().any(|range| range.end > start)
}

pub(crate) fn mask_ranges(
    source: &str,
    strings: &[Range<usize>],
    comments: &[Range<usize>],
    preserve: &[Range<usize>],
) -> String {
    let mut bytes = source.as_bytes().to_vec();
    for range in strings {
        for (index, byte) in bytes
            .get_mut(range.clone())
            .unwrap_or(&mut [])
            .iter_mut()
            .enumerate()
        {
            let offset = range.start + index;
            if *byte != b'\n'
                && !preserve.iter().any(|p| p.start <= offset && offset < p.end)
            {
                *byte = b'x';
            }
        }
    }
    for range in comments {
        for byte in bytes.get_mut(range.clone()).unwrap_or(&mut []) {
            if *byte != b'\n' {
                *byte = b' ';
            }
        }
    }
    String::from_utf8(bytes).unwrap_or_else(|_| source.to_owned())
}

pub(crate) fn count_identifier(source: &str, name: &str) -> usize {
    if name.is_empty() {
        return 0;
    }
    let mut count = 0;
    let mut offset = 0;
    while let Some(index) = source[offset..].find(name) {
        let start = offset + index;
        let end = start + name.len();
        let before_ok = source[..start]
            .chars()
            .next_back()
            .is_none_or(|c| !c.is_alphanumeric() && c != '_');
        let after_ok = source[end..]
            .chars()
            .next()
            .is_none_or(|c| !c.is_alphanumeric() && c != '_');
        if before_ok && after_ok {
            count += 1;
        }
        offset = end;
    }
    count
}

pub(crate) fn contains_word(text: &str, word: &str) -> bool {
    let Ok(regex) = Regex::new(&format!(r"\b{}\b", regex::escape(word))) else {
        return false;
    };
    regex.is_match(text)
}
