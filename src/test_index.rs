use tree_sitter::{Node, Tree};

use crate::analyze;
use crate::language::Language;

pub fn collect_tests(language: Language, source: &str, tree: &Tree) -> Vec<String> {
    let mut tests = Vec::new();
    visit(tree.root_node(), &mut |node| match language {
        Language::Python => collect_python_test(source, node, &mut tests),
        Language::Rust => collect_rust_test(source, node, &mut tests),
        _ if language.is_script() => collect_script_test(source, node, &mut tests),
        _ => {}
    });
    tests
}

pub fn is_test_file_path(relative: &str) -> bool {
    let path = relative.replace('\\', "/");
    let name = path.rsplit('/').next().unwrap_or(&path);
    path.split('/')
        .any(|part| part == "tests" || part == "test")
        || name.starts_with("test_")
        || name.ends_with("_test.py")
        || name.ends_with("_test.go")
        || name.ends_with("_test.rs")
        || name.contains(".test.")
        || name.contains(".spec.")
}

fn visit(node: Node, callback: &mut impl FnMut(Node)) {
    callback(node);
    for index in 0..node.named_child_count() {
        if let Some(child) = node.named_child(index as u32) {
            visit(child, callback);
        }
    }
}

fn collect_python_test(source: &str, node: Node, tests: &mut Vec<String>) {
    if node.kind() != "function_definition" {
        return;
    }
    let Some(name) =
        analyze::function_name(node).map(|name_node| analyze::node_text(source, name_node))
    else {
        return;
    };
    if !name.starts_with("test_") {
        return;
    }
    tests.push(name.to_owned());
    if let Some(class_name) = enclosing_class_name(source, node)
        && class_name.starts_with("Test")
    {
        tests.push(format!("{class_name}.{name}"));
        tests.push(format!("{class_name}::{name}"));
    }
}

fn enclosing_class_name<'a>(source: &'a str, node: Node) -> Option<&'a str> {
    let block = node.parent()?;
    if block.kind() != "block" {
        return None;
    }
    let class = block.parent()?;
    if class.kind() != "class_definition" {
        return None;
    }
    class
        .child_by_field_name("name")
        .map(|name| analyze::node_text(source, name))
}

fn collect_rust_test(source: &str, node: Node, tests: &mut Vec<String>) {
    if node.kind() != "function_item" {
        return;
    }
    let mut sibling = node.prev_named_sibling();
    let mut is_test = false;
    while let Some(current) = sibling {
        if current.kind() != "attribute_item" {
            break;
        }
        let text = analyze::node_text(source, current);
        if text.contains("#[test]") || text.contains("::test]") || text.contains("#[rstest") {
            is_test = true;
        }
        sibling = current.prev_named_sibling();
    }
    if is_test && let Some(name) = analyze::function_name(node) {
        tests.push(analyze::node_text(source, name).to_owned());
    }
}

fn collect_script_test(source: &str, node: Node, tests: &mut Vec<String>) {
    if node.kind() != "call_expression" {
        return;
    }
    let Some(callee) = node
        .child_by_field_name("function")
        .map(|function| analyze::node_text(source, function))
    else {
        return;
    };
    let root = callee.split('.').next().unwrap_or("");
    if !matches!(root, "test" | "it") {
        return;
    }
    let Some(arguments) = node.child_by_field_name("arguments") else {
        return;
    };
    for index in 0..arguments.named_child_count() {
        let Some(argument) = arguments.named_child(index as u32) else {
            continue;
        };
        if argument.kind() == "comment" {
            continue;
        }
        if matches!(argument.kind(), "string" | "template_string") {
            let raw = analyze::node_text(source, argument);
            let title = raw
                .trim_matches(['\'', '"', '`'])
                .split('\n')
                .next()
                .unwrap_or("")
                .trim();
            if !title.is_empty() {
                tests.push(title.to_owned());
            }
        }
        break;
    }
}
