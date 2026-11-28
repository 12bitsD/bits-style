use tree_sitter::Node;

use super::ast::node_text;
use super::text_util::contains_word;

pub(super) fn is_rust_allowed_wildcard(
    relative: &str,
    source: &str,
    node: Node,
    text: &str,
) -> bool {
    let trimmed = text.trim_start();
    let scoped = trimmed.starts_with("pub(") && trimmed.contains(") use ");
    if trimmed.starts_with("pub use ")
        || scoped
        || trimmed.contains("::prelude::*")
        || trimmed.contains("::lib::*")
        || in_cfg_test_module(source, node)
        || crate::test_index::is_test_file_path(relative)
    {
        return true;
    }
    let target = trimmed.trim_end_matches(';').trim_end();
    if matches!(target, "use super::*" | "use self::*")
        && in_inline_mod_item(node)
    {
        return true;
    }
    target.rsplit_once("::").is_some_and(|(prefix, _)| {
        prefix
            .rsplit("::")
            .next()
            .unwrap_or("")
            .trim()
            .chars()
            .next()
            .is_some_and(char::is_uppercase)
    })
}

fn in_inline_mod_item(node: Node) -> bool {
    let mut current = node.parent();
    while let Some(parent) = current {
        if parent.kind() == "mod_item"
            && parent.child_by_field_name("body").is_some()
        {
            return true;
        }
        current = parent.parent();
    }
    false
}

fn is_test_cfg(source: &str, node: Node) -> bool {
    let text = node_text(source, node);
    text.contains("cfg(") && contains_word(text, "test")
}

fn has_leading_inner_test_cfg(source: &str, container: Node) -> bool {
    for index in 0..container.named_child_count() {
        let Some(child) = container.named_child(index as u32) else {
            continue;
        };
        if child.kind() != "inner_attribute_item" {
            break;
        }
        if is_test_cfg(source, child) {
            return true;
        }
    }
    false
}

fn has_outer_test_cfg(source: &str, item: Node) -> bool {
    let mut sibling = item.prev_named_sibling();
    while let Some(attr) = sibling {
        if attr.kind() != "attribute_item" {
            break;
        }
        if is_test_cfg(source, attr) {
            return true;
        }
        sibling = attr.prev_named_sibling();
    }
    false
}

fn in_cfg_test_module(source: &str, node: Node) -> bool {
    let mut current = Some(node);
    while let Some(item) = current {
        let matched = match item.kind() {
            "source_file" => {
                has_outer_test_cfg(source, item)
                    || has_leading_inner_test_cfg(source, item)
            }
            "mod_item" => {
                has_outer_test_cfg(source, item)
                    || item
                        .child_by_field_name("body")
                        .is_some_and(|b| has_leading_inner_test_cfg(source, b))
            }
            _ => false,
        };
        if matched {
            return true;
        }
        current = item.parent();
    }
    false
}
