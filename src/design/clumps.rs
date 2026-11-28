use std::collections::{BTreeMap, BTreeSet};

use crate::model::Violation;

pub(crate) type FunctionParams = (String, usize, usize, Vec<String>);
type FunctionSite = (String, usize, usize);
type ParamTriple = (String, String, String);

const COMMON_PARAMS: &[&str] = &[
    "self", "cls", "this", "ctx", "env", "key", "val", "value", "name", "path",
    "data", "item", "items", "text", "node", "args", "kwargs", "opts",
    "options", "state", "config", "source", "target", "index", "count",
    "limit", "error", "start", "end", "left", "right", "width", "height",
    "lines", "line", "root", "file", "out", "err", "req", "res", "len", "now",
    "row", "raw", "conn", "params", "visitor",
];

pub(crate) fn is_meaningful_clump_param(name: &str) -> bool {
    name.len() >= 3 && !name.starts_with('_') && !COMMON_PARAMS.contains(&name)
}

pub(crate) fn analyze_data_clumps(
    relative: &str,
    functions: &[FunctionParams],
    violations: &mut Vec<Violation>,
) {
    if functions.len() < 3 {
        return;
    }
    let ranked = rank_param_triples(functions);
    let mut reported: BTreeSet<String> = BTreeSet::new();
    for ((a, b, c), sites) in ranked {
        if reported.contains(&a)
            && reported.contains(&b)
            && reported.contains(&c)
        {
            continue;
        }
        let group = expand_group(&[a, b, c], &sites, functions);
        reported.extend(group.iter().cloned());
        if let Some(violation) = build_clump_violation(relative, &group, &sites)
        {
            violations.push(violation);
        }
    }
}

fn rank_param_triples(
    functions: &[FunctionParams],
) -> Vec<(ParamTriple, Vec<FunctionSite>)> {
    let mut triple_sites: BTreeMap<ParamTriple, Vec<FunctionSite>> =
        BTreeMap::new();
    for entry in functions {
        record_function_triples(entry, &mut triple_sites);
    }
    let mut ranked: Vec<(ParamTriple, Vec<FunctionSite>)> = triple_sites
        .into_iter()
        .filter(|(_, sites)| distinct_name_count(sites) >= 3)
        .collect();
    ranked
        .sort_by(|a, b| b.1.len().cmp(&a.1.len()).then_with(|| a.0.cmp(&b.0)));
    ranked
}

fn record_function_triples(
    entry: &FunctionParams,
    triple_sites: &mut BTreeMap<ParamTriple, Vec<FunctionSite>>,
) {
    let (fn_name, line, col, params) = entry;
    let unique = sorted_unique(params);
    if unique.len() < 3 {
        return;
    }
    for i in 0..unique.len() {
        for j in (i + 1)..unique.len() {
            for k in (j + 1)..unique.len() {
                let triple =
                    (unique[i].clone(), unique[j].clone(), unique[k].clone());
                triple_sites.entry(triple).or_default().push((
                    fn_name.clone(),
                    *line,
                    *col,
                ));
            }
        }
    }
}

fn sorted_unique(params: &[String]) -> Vec<String> {
    let set: BTreeSet<&str> = params.iter().map(String::as_str).collect();
    set.into_iter().map(str::to_owned).collect()
}

fn distinct_name_count(sites: &[FunctionSite]) -> usize {
    let names: BTreeSet<&str> =
        sites.iter().map(|(n, _, _)| n.as_str()).collect();
    names.len()
}

fn expand_group(
    seed: &[String; 3],
    sites: &[FunctionSite],
    functions: &[FunctionParams],
) -> Vec<String> {
    let site_lines: BTreeSet<usize> =
        sites.iter().map(|(_, l, _)| *l).collect();
    let mut group = seed.to_vec();
    for (_, line, _, params) in functions {
        if !site_lines.contains(line) {
            continue;
        }
        append_shared_params(&mut group, params, &site_lines, functions);
    }
    group.sort();
    group
}

fn append_shared_params(
    group: &mut Vec<String>,
    params: &[String],
    site_lines: &BTreeSet<usize>,
    functions: &[FunctionParams],
) {
    for extra in params {
        if group.contains(extra) {
            continue;
        }
        let shared = functions
            .iter()
            .filter(|(_, l, _, ps)| {
                site_lines.contains(l) && ps.contains(extra)
            })
            .count();
        if shared >= 3 {
            group.push(extra.clone());
        }
    }
}

fn build_clump_violation(
    relative: &str,
    group: &[String],
    sites: &[FunctionSite],
) -> Option<Violation> {
    let (_, first_line, first_col) = sites.first()?;
    let fn_list: Vec<&str> =
        sites.iter().take(4).map(|(n, _, _)| n.as_str()).collect();
    let group_str = group.join(", ");
    let mut violation = Violation::new(
        "BCS004",
        relative,
        *first_line,
        *first_col,
        "parameters repeatedly travel together across functions (data clump)",
        format!("({group_str}) in {} functions", sites.len()),
        "group_into_value_object",
        false,
    );
    violation.mark_taste_warning("state-gain");
    violation.prescription = Some(format!(
        "group co-occurring parameters ({group_str}) used by {} into a frozen dataclass/struct",
        fn_list.join(", ")
    ));
    Some(violation)
}
