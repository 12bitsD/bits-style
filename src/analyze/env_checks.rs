use tree_sitter::Node;

use crate::language::Language;

use super::ast::{
    find_parameters, function_name, is_function, node_text, parameter_name,
};
use super::naming::parameter_nodes;
use super::types::{Analyzer, Diag};

pub(crate) fn is_type_alias(
    language: Language,
    source: &str,
    node: Node,
) -> bool {
    if language != Language::Python
        || node.child_by_field_name("type").is_some()
    {
        return false;
    }
    let text = node_text(source, node);
    let name = text.split('=').next().unwrap_or("").trim();
    let value = text.split_once('=').map_or("", |parts| parts.1.trim());
    let unqualified = strip_module_prefix(value);
    if has_type_alias_prefix(value, unqualified) || is_builtin_type_name(value)
    {
        return true;
    }
    name.chars()
        .next()
        .is_some_and(|c| c.is_uppercase() || c == '_')
        && value.chars().next().is_some_and(char::is_uppercase)
        && value.chars().all(|c| c.is_alphanumeric() || c == '_')
}

fn strip_module_prefix(value: &str) -> &str {
    value.split_once('.').map_or(value, |(module, rest)| {
        if module.chars().all(|c| c.is_alphanumeric() || c == '_') {
            rest
        } else {
            value
        }
    })
}

fn has_type_alias_prefix(value: &str, unqualified: &str) -> bool {
    const PREFIXES: [&str; 30] = [
        "Annotated[",
        "Literal[",
        "Callable[",
        "Union[",
        "Optional[",
        "TypeVar(",
        "NewType(",
        "Type[",
        "list[",
        "dict[",
        "tuple[",
        "set[",
        "frozenset[",
        "Sequence[",
        "Mapping[",
        "MutableMapping[",
        "Iterable[",
        "Iterator[",
        "Awaitable[",
        "Coroutine[",
        "Protocol",
        "TypedDict",
        "NamedTuple",
        "TypeAlias",
        "TypeAliasType",
        "LiteralString",
        "Self",
        "ClassVar",
        "Final",
        "type ",
    ];
    PREFIXES
        .iter()
        .any(|p| value.starts_with(p) || unqualified.starts_with(p))
}

fn is_builtin_type_name(value: &str) -> bool {
    matches!(
        value,
        "int"
            | "str"
            | "float"
            | "bool"
            | "bytes"
            | "dict"
            | "list"
            | "set"
            | "tuple"
            | "Any"
            | "Self"
    )
}

pub(super) fn in_python_main_guard(source: &str, node: Node) -> bool {
    let mut current = node.parent();
    while let Some(parent) = current {
        if parent.kind() == "if_statement"
            && let Some(condition) = parent.child_by_field_name("condition")
        {
            let text = node_text(source, condition);
            if text.contains("__name__") && text.contains("__main__") {
                return true;
            }
        }
        if matches!(parent.kind(), "function_definition" | "class_definition") {
            return false;
        }
        current = parent.parent();
    }
    false
}

pub(super) fn is_optional_import_fallback(
    source: &str,
    node: Node,
    value: &str,
) -> bool {
    if value != "None" {
        return false;
    }
    let mut current = node.parent();
    while let Some(parent) = current {
        if parent.kind() == "except_clause" {
            let text = node_text(source, parent);
            return text.starts_with("except ImportError")
                || text.starts_with("except ModuleNotFoundError")
                || (text.starts_with("except (")
                    && text.contains("ImportError"));
        }
        if matches!(parent.kind(), "try_statement" | "function_definition") {
            return false;
        }
        current = parent.parent();
    }
    false
}

pub(crate) fn analyze_hidden_input(analyzer: &mut Analyzer<'_>, node: Node) {
    let text = node_text(analyzer.source, node);
    let callee = node
        .child_by_field_name("function")
        .map_or("", |func| node_text(analyzer.source, func));
    let hidden = match analyzer.language {
        Language::Python => {
            text.contains("os.environ") || text.contains("os.getenv")
        }
        Language::Rust => {
            callee.ends_with("env::var")
                || callee.ends_with("env::var_os")
                || callee.ends_with("env::args")
                || callee.ends_with("env::args_os")
        }
        Language::Go => {
            text.starts_with("os.Getenv") || text.starts_with("os.Environ")
        }
        _ if analyzer.language.is_script() => text.contains("process.env"),
        _ => text.starts_with("getenv(") || text.contains("std::getenv"),
    };
    if hidden
        && !in_env_composition_boundary(
            analyzer.language,
            analyzer.source,
            node,
        )
    {
        analyzer.emit_taste(
            Diag::new(
                node,
                "BCS011",
                "environment values should be read at the boundary and passed \
                 explicitly",
                (text.lines().next().unwrap_or(text), "explicit_input"),
            ),
            "state-gain",
        );
    }
}

fn in_env_composition_boundary(
    language: Language,
    source: &str,
    node: Node,
) -> bool {
    if language == Language::Python && in_python_main_guard(source, node) {
        return true;
    }
    let mut current = node.parent();
    while let Some(ancestor) = current {
        if is_function(ancestor.kind()) {
            return is_env_boundary_function(language, source, ancestor);
        }
        current = ancestor.parent();
    }
    false
}

fn is_env_boundary_function(
    language: Language,
    source: &str,
    ancestor: Node,
) -> bool {
    let name =
        function_name(ancestor).map_or("", |n| node_text(source, n).trim());
    if matches!(name, "main" | "run_cli" | "cli_main") {
        return true;
    }
    let Some(parameters) = find_parameters(ancestor) else {
        return false;
    };
    parameter_nodes(language, source, parameters)
        .into_iter()
        .filter_map(|param| parameter_name(source, param))
        .any(|param| {
            matches!(
                param.as_str(),
                "env" | "environ" | "env_override" | "env_map" | "envVars"
            )
        })
}
