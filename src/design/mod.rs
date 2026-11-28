mod adjacent;
mod clumps;
mod params;
mod patterns;

use std::ops::Range;

use tree_sitter::{Node, Tree};

use crate::analyze::{
    find_parameters, function_name, is_function, mask_ranges, node_text,
    parameter_name, parameter_nodes, visit,
};
use crate::language::Language;
use crate::model::Violation;

use adjacent::analyze_adjacent_same_type_params;
use clumps::{FunctionParams, analyze_data_clumps, is_meaningful_clump_param};
use params::analyze_tramp_data;
use patterns::{
    DispatchChain, analyze_repeated_dispatch, analyze_thin_wrappers,
    extract_dispatch_chain,
};

pub struct DesignInput<'a> {
    pub language: Language,
    pub relative: &'a str,
    pub source: &'a str,
    pub tree: &'a Tree,
    pub strings: &'a [Range<usize>],
    pub comments: &'a [Range<usize>],
    pub interpolations: &'a [Range<usize>],
}

pub(crate) struct DesignContext<'a> {
    pub language: Language,
    pub relative: &'a str,
    pub source: &'a str,
    pub violations: &'a mut Vec<Violation>,
}

pub fn analyze_design_mechanics(
    input: &DesignInput<'_>,
    violations: &mut Vec<Violation>,
) {
    if crate::test_index::is_test_file_path(input.relative) {
        return;
    }
    let root = input.tree.root_node();
    let masked = mask_ranges(
        input.source,
        input.strings,
        input.comments,
        input.interpolations,
    );
    let mut ctx = DesignContext {
        language: input.language,
        relative: input.relative,
        source: input.source,
        violations,
    };
    let mut function_params: Vec<FunctionParams> = Vec::new();
    let mut dispatch_chains: Vec<DispatchChain> = Vec::new();
    visit(root, &mut |node| {
        if is_function(node.kind()) {
            inspect_function(&mut ctx, node, &mut function_params);
        } else if node.kind() == "if_statement"
            && let Some(chain) = extract_dispatch_chain(ctx.source, node)
        {
            dispatch_chains.push(chain);
        }
    });
    analyze_data_clumps(ctx.relative, &function_params, ctx.violations);
    analyze_repeated_dispatch(ctx.relative, &dispatch_chains, ctx.violations);
    analyze_thin_wrappers(&mut ctx, root, &masked);
}

fn inspect_function(
    ctx: &mut DesignContext<'_>,
    node: Node,
    function_params: &mut Vec<FunctionParams>,
) {
    let Some(params_node) = find_parameters(node) else {
        return;
    };
    let fn_name =
        function_name(node).map(|n| node_text(ctx.source, n).trim().to_owned());
    let params = parameter_nodes(ctx.language, ctx.source, params_node);
    analyze_tramp_data(ctx, node, &params);
    analyze_adjacent_same_type_params(
        ctx,
        fn_name.as_deref(),
        params_node,
        &params,
    );
    if let Some(record) =
        extract_clump_record(ctx.source, node, fn_name.as_deref(), &params)
    {
        function_params.push(record);
    }
}

fn extract_clump_record(
    source: &str,
    node: Node,
    fn_name: Option<&str>,
    params: &[Node],
) -> Option<FunctionParams> {
    let name = fn_name?;
    if matches!(name, "__init__" | "__new__" | "constructor") {
        return None;
    }
    let names: Vec<String> = params
        .iter()
        .filter_map(|p| parameter_name(source, *p))
        .filter(|p| is_meaningful_clump_param(p))
        .collect();
    if names.len() < 2 {
        return None;
    }
    Some((
        name.to_owned(),
        node.start_position().row + 1,
        node.start_position().column + 1,
        names,
    ))
}
