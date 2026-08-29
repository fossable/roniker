use super::ts_utils::{child_by_kind, field_name, node_at_position, struct_name};
use tower_lsp::lsp_types::Position;
use tree_sitter::{Node, Tree};

/// Represents the nesting context at a cursor position
#[derive(Debug, Clone)]
pub struct TypeContext {
    /// The type name we're currently inside (e.g., "User", "Post")
    pub type_name: String,
}

/// Find the nested type context at a cursor position using tree-sitter
/// Returns a stack of type contexts from outermost to innermost
pub fn find_type_context_at_position(
    tree: &Tree,
    content: &str,
    position: Position,
) -> Vec<TypeContext> {
    let mut contexts = Vec::new();
    if let Some(mut current) = node_at_position(tree, content, position) {
        // Walk up the tree to collect all struct contexts
        loop {
            if current.kind() == "struct"
                && let Some(type_name) = struct_name(&current, content)
            {
                contexts.push(TypeContext {
                    type_name: type_name.to_string(),
                });
            }

            match current.parent() {
                Some(parent) => current = parent,
                None => break,
            }
        }
    }

    // Reverse to get outermost to innermost
    contexts.reverse();
    contexts
}

/// Get the field name at a specific position in RON content using tree-sitter
pub fn get_field_at_position(tree: &Tree, content: &str, position: Position) -> Option<String> {
    // Walk up to find a field node
    let mut current = node_at_position(tree, content, position)?;
    loop {
        if let Some(name) = field_name(&current, content) {
            return Some(name.to_string());
        }

        match current.parent() {
            Some(parent) => current = parent,
            None => break,
        }
    }

    None
}

/// Find the current variant context (enum variant name) at a position
pub fn find_current_variant_context(
    tree: &Tree,
    content: &str,
    position: Position,
) -> Option<String> {
    // Walk up to find the innermost struct node with a name
    let mut current = node_at_position(tree, content, position)?;
    loop {
        if current.kind() == "struct" {
            // Check if this struct has a name (making it a variant)
            if let Some(name) = struct_name(&current, content) {
                // Make sure it's actually a variant by checking if it's uppercase
                if name.chars().next()?.is_uppercase() {
                    return Some(name.to_string());
                }
            }
        }

        match current.parent() {
            Some(parent) => current = parent,
            None => break,
        }
    }

    None
}

/// Get the containing field context by finding the parent field
/// For example: "post_type: Detailed(\n    length: 1" - when on "length" line, returns "post_type"
pub fn get_containing_field_context(
    tree: &Tree,
    content: &str,
    position: Position,
) -> Option<String> {
    // Walk up the tree to find the parent field that contains a struct which
    // contains our current field
    let mut current = node_at_position(tree, content, position)?;
    let mut found_current_field = false;

    loop {
        if current.kind() == "field" {
            if found_current_field {
                // This is the containing field - extract its name
                if let Some(name) = field_name(&current, content) {
                    return Some(name.to_string());
                }
            } else {
                // This is the first field we found (the one we're in)
                found_current_field = true;
            }
        }

        match current.parent() {
            Some(parent) => current = parent,
            None => break,
        }
    }

    None
}

/// Information about a variant field location in RON content
#[derive(Debug, Clone)]
pub struct VariantFieldLocation {
    pub line_idx: usize,
    pub variant_name: String,
    pub containing_field_name: String,
    pub field_at_position: Option<String>,
}

/// Scan through content and find all variant field locations
pub fn find_all_variant_field_locations(tree: &Tree, content: &str) -> Vec<VariantFieldLocation> {
    let mut locations = Vec::new();
    let root = tree.root_node();

    // Walk the tree to find all field nodes that are inside struct variants
    visit_fields(&root, content, &mut locations);

    locations
}

/// Recursively visit nodes to find field locations inside variants
fn visit_fields(node: &Node, content: &str, locations: &mut Vec<VariantFieldLocation>) {
    // Check if this is a struct (potential variant)
    if node.kind() == "struct"
        && let Some(variant_name) = struct_name(node, content)
    {
        // This is a named struct, check if it's a variant (uppercase start)
        if variant_name
            .chars()
            .next()
            .is_some_and(|c| c.is_uppercase())
        {
            // Look for the containing field by checking parent
            let containing_field_name = find_parent_field_name(node, content);

            // Now collect all fields inside this variant
            collect_fields_in_node(
                node,
                content,
                variant_name,
                &containing_field_name,
                locations,
            );
        }
    }

    // Recurse into children
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        visit_fields(&child, content, locations);
    }
}

/// Find the parent field name of a node
fn find_parent_field_name(node: &Node, content: &str) -> Option<String> {
    let mut current = *node;
    while let Some(parent) = current.parent() {
        if let Some(name) = field_name(&parent, content) {
            return Some(name.to_string());
        }
        current = parent;
    }
    None
}

/// Collect all fields in a node
fn collect_fields_in_node(
    node: &Node,
    content: &str,
    variant_name: &str,
    containing_field_name: &Option<String>,
    locations: &mut Vec<VariantFieldLocation>,
) {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == "field" {
            let line_idx = child.start_position().row;

            // Extract field name
            let field_at_position = field_name(&child, content).map(|s| s.to_string());

            if let Some(containing_field) = containing_field_name {
                locations.push(VariantFieldLocation {
                    line_idx,
                    variant_name: variant_name.to_string(),
                    containing_field_name: containing_field.clone(),
                    field_at_position,
                });
            }
        }

        // Recurse for nested structures
        collect_fields_in_node(
            &child,
            content,
            variant_name,
            containing_field_name,
            locations,
        );
    }
}

/// Parse RON structure to get all field names present at the top level
pub fn extract_fields_from_ron(tree: &Tree, content: &str) -> Vec<String> {
    let mut fields = std::collections::HashSet::new();
    let root = tree.root_node();

    // Find the first struct node (the top-level value)
    if let Some(struct_node) = child_by_kind(&root, "struct") {
        // Collect only direct child fields of this struct
        collect_direct_field_names(&struct_node, content, &mut fields);
    }

    fields.into_iter().collect()
}

/// Collect only direct child field names from a struct node (not recursively)
fn collect_direct_field_names(
    node: &Node,
    content: &str,
    fields: &mut std::collections::HashSet<String>,
) {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if let Some(name) = field_name(&child, content) {
            fields.insert(name.to_string());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::ts_utils::RonParser;

    fn parse(content: &str) -> Tree {
        RonParser::new().parse(content).unwrap()
    }

    #[test]
    fn test_parse_simple_struct() {
        let content = r#"MyStruct(
    name: "test",
    age: 30,
)"#;
        let mut parser = RonParser::new();
        let tree = parser.parse(content);
        assert!(tree.is_some());
    }

    #[test]
    fn test_find_type_context_nested() {
        let content = r#"PostReference(Post(
    id: 42,
    author: User(
        name: "Alice",
    ),
))"#;
        // Position inside User
        let contexts = find_type_context_at_position(&parse(content), content, Position::new(3, 20));
        assert_eq!(contexts.len(), 3);
        assert_eq!(contexts[0].type_name, "PostReference");
        assert_eq!(contexts[1].type_name, "Post");
        assert_eq!(contexts[2].type_name, "User");
    }

    #[test]
    fn test_get_field_at_position() {
        let content = r#"MyStruct(
    name: "test",
    age: 30,
)"#;
        // Position on "name" field
        let field = get_field_at_position(&parse(content), content, Position::new(1, 8));
        assert_eq!(field, Some("name".to_string()));
    }

    #[test]
    fn test_find_current_variant_context() {
        let content = r#"Detailed(
    length: 1,
)"#;
        let variant = find_current_variant_context(&parse(content), content, Position::new(1, 12));
        assert_eq!(variant, Some("Detailed".to_string()));
    }

    #[test]
    fn test_extract_fields_from_ron() {
        let content = r#"MyStruct(
    name: "test",
    age: 30,
    items: [],
)"#;
        let fields = extract_fields_from_ron(&parse(content), content);
        assert!(fields.contains(&"name".to_string()));
        assert!(fields.contains(&"age".to_string()));
        assert!(fields.contains(&"items".to_string()));
    }

    #[test]
    fn test_enum_variant_field_detection() {
        let content = r#"Post(
    id: 42,
    post_type: Detailed(
        length: 1,
    ),
)"#;
        let position = Position::new(3, 16);

        let tree = parse(content);
        let variant = find_current_variant_context(&tree, content, position);
        assert_eq!(variant, Some("Detailed".to_string()));

        let containing_field = get_containing_field_context(&tree, content, position);
        assert_eq!(containing_field, Some("post_type".to_string()));
    }
}
