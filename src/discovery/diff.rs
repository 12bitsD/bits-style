use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::process::Command;

use crate::discovery::LineScope;

pub(crate) fn git_diff_lines(
    root: &Path,
    base: &str,
    pathspecs: &[String],
) -> Result<BTreeMap<String, LineScope>, String> {
    validate_commit(root, base)?;
    let output = Command::new("git")
        .current_dir(root)
        .args([
            "-c",
            "core.quotePath=false",
            "diff",
            "--unified=0",
            "--no-color",
            "--no-ext-diff",
            "--no-textconv",
            "--src-prefix=a/",
            "--dst-prefix=b/",
            "--end-of-options",
            base,
            "--",
        ])
        .args(pathspecs)
        .output()
        .map_err(|error| format!("failed to run git diff: {error}"))?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_owned());
    }
    parse_diff_lines(&String::from_utf8_lossy(&output.stdout))
}

fn validate_commit(root: &Path, base: &str) -> Result<(), String> {
    let revision = Command::new("git")
        .current_dir(root)
        .args([
            "rev-parse",
            "--verify",
            "--quiet",
            "--end-of-options",
            &format!("{base}^{{commit}}"),
        ])
        .output()
        .map_err(|error| format!("failed to validate diff commit: {error}"))?;
    if !revision.status.success() {
        return Err(format!("invalid diff commit: {base}"));
    }
    Ok(())
}

pub(crate) fn parse_diff_lines(
    text: &str,
) -> Result<BTreeMap<String, LineScope>, String> {
    let mut files: BTreeMap<String, LineScope> = BTreeMap::new();
    let mut current: Option<String> = None;
    let mut lines = text.lines();
    while let Some(line) = lines.next() {
        if let Some(path) = line.strip_prefix("+++ ") {
            current = register_diff_target(path, &mut files);
            continue;
        }
        if line.starts_with("@@") {
            apply_hunk(line, &mut lines, current.as_deref(), &mut files)?;
        }
    }
    Ok(files)
}

fn register_diff_target(
    raw: &str,
    files: &mut BTreeMap<String, LineScope>,
) -> Option<String> {
    let path = raw.trim_end_matches(['\t', '\r']);
    if path == "/dev/null" {
        return None;
    }
    let decoded = decode_git_path(path);
    let rel = decoded.strip_prefix("b/").unwrap_or(&decoded).to_owned();
    files
        .entry(rel.clone())
        .or_insert_with(|| LineScope::Added(BTreeSet::new()));
    Some(rel)
}

struct HunkCursor {
    old_left: usize,
    new_left: usize,
    new_line: usize,
}

fn apply_hunk<'a>(
    header: &str,
    lines: &mut impl Iterator<Item = &'a str>,
    current: Option<&str>,
    files: &mut BTreeMap<String, LineScope>,
) -> Result<(), String> {
    let (old_left, new_line, new_left) = parse_hunk(header)
        .ok_or_else(|| format!("invalid diff hunk: {header}"))?;
    if new_left > 0 && current.is_none() {
        return Err(format!("diff hunk has no destination file: {header}"));
    }
    let mut cursor = HunkCursor {
        old_left,
        new_left,
        new_line,
    };
    let target = current.and_then(|path| files.get_mut(path));
    let mut added = match target {
        Some(LineScope::Added(set)) => Some(set),
        _ => None,
    };
    while cursor.old_left > 0 || cursor.new_left > 0 {
        let body = lines.next().ok_or_else(|| {
            format!("truncated hunk for {}", current.unwrap_or("/dev/null"))
        })?;
        step_hunk_line(header, body, &mut cursor, added.as_deref_mut())?;
    }
    Ok(())
}

fn step_hunk_line(
    header: &str,
    body: &str,
    cursor: &mut HunkCursor,
    added: Option<&mut BTreeSet<usize>>,
) -> Result<(), String> {
    match body.as_bytes().first() {
        Some(b' ') => {
            if cursor.old_left == 0 || cursor.new_left == 0 {
                return Err(format!(
                    "diff hunk line counts do not match: {header}"
                ));
            }
            cursor.old_left -= 1;
            cursor.new_left -= 1;
            cursor.new_line += 1;
        }
        Some(b'+') => {
            if cursor.new_left == 0 {
                return Err(format!(
                    "diff hunk has too many added lines: {header}"
                ));
            }
            if let Some(set) = added {
                set.insert(cursor.new_line);
            }
            cursor.new_left -= 1;
            cursor.new_line += 1;
        }
        Some(b'-') => {
            if cursor.old_left == 0 {
                return Err(format!(
                    "diff hunk has too many deleted lines: {header}"
                ));
            }
            cursor.old_left -= 1;
        }
        Some(b'\\') => {}
        _ => return Err(format!("invalid diff hunk body: {body}")),
    }
    Ok(())
}

pub(crate) fn decode_git_path(path: &str) -> String {
    if !(path.starts_with('"') && path.ends_with('"') && path.len() >= 2) {
        return path.to_owned();
    }
    let bytes = &path.as_bytes()[1..path.len() - 1];
    let mut output = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] != b'\\' {
            output.push(bytes[index]);
            index += 1;
            continue;
        }
        index += 1;
        if let Some((decoded, next)) = decode_escape(bytes, index) {
            output.push(decoded);
            index = next;
        }
    }
    String::from_utf8_lossy(&output).into_owned()
}

fn decode_escape(bytes: &[u8], index: usize) -> Option<(u8, usize)> {
    let byte = *bytes.get(index)?;
    let decoded = match byte {
        b'n' => b'\n',
        b't' => b'\t',
        b'r' => b'\r',
        b'\\' => b'\\',
        b'"' => b'"',
        digit @ b'0'..=b'7' => return Some(decode_octal(bytes, index, digit)),
        other => other,
    };
    Some((decoded, index + 1))
}

fn decode_octal(bytes: &[u8], mut index: usize, first: u8) -> (u8, usize) {
    let mut value = first - b'0';
    for _ in 0..2 {
        if let Some(&next) = bytes.get(index + 1)
            && matches!(next, b'0'..=b'7')
        {
            index += 1;
            value = value * 8 + (next - b'0');
        }
    }
    (value, index + 1)
}

fn parse_hunk(line: &str) -> Option<(usize, usize, usize)> {
    let mut parts = line.strip_prefix("@@ ")?.split_whitespace();
    let old = parts.next()?.strip_prefix('-')?;
    let new = parts.next()?.strip_prefix('+')?;
    if parts.next()? != "@@" {
        return None;
    }
    let (_, old_count) = parse_hunk_range(old)?;
    let (new_start, new_count) = parse_hunk_range(new)?;
    Some((old_count, new_start, new_count))
}

fn parse_hunk_range(range: &str) -> Option<(usize, usize)> {
    let (start, count) = range.split_once(',').unwrap_or((range, "1"));
    Some((start.parse().ok()?, count.parse().ok()?))
}
