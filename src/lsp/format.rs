/// Tree-sitter based RON formatter with comment preservation
/// This formatter uses the AST to properly handle formatting while preserving comments
use super::ts_utils::{self, RonParser};
use std::collections::HashSet;
use tree_sitter::Node;

/// Represents a comment found in the source
#[derive(Debug, Clone)]
struct Comment {
    text: String,
    /// The byte position where this comment starts
    start_byte: usize,
    /// The byte position where this comment ends
    end_byte: usize,
    /// True if this comment appears on the same line as code before it
    is_trailing: bool,
}

/// Collect comments that are direct children of a container node
/// Returns comments with their positions relative to sibling nodes
fn collect_inner_comments(node: &Node, content: &str) -> Vec<Comment> {
    let mut result = Vec::new();
    let mut cursor = node.walk();
    let children: Vec<_> = node.children(&mut cursor).collect();

    for (i, child) in children.iter().enumerate() {
        let kind = child.kind();
        if kind == "line_comment" || kind == "block_comment" {
            if let Some(text) = ts_utils::node_text(child, content) {
                let start_byte = child.start_byte();

                // Determine if this is a trailing comment by checking if there's
                // a non-comment, non-punctuation sibling before it on the same line
                let is_trailing = is_trailing_comment_in_context(
                    content,
                    start_byte,
                    &children[..i],
                );

                result.push(Comment {
                    text: text.to_string(),
                    start_byte: child.start_byte(),
                    end_byte: child.end_byte(),
                    is_trailing,
                });
            }
        }
    }

    result
}

/// Determine if a comment is trailing by checking if there's a significant sibling
/// (field, value, etc.) before it on the same line
fn is_trailing_comment_in_context(
    content: &str,
    comment_start: usize,
    siblings_before: &[Node],
) -> bool {
    // Find the line start for the comment
    let before = &content[..comment_start];
    let line_start = before.rfind('\n').map(|p| p + 1).unwrap_or(0);

    // Check if any significant sibling ends on this same line (after line_start)
    for sibling in siblings_before.iter().rev() {
        let kind = sibling.kind();
        // Skip punctuation like ( ) , :
        if !sibling.is_named() {
            continue;
        }
        // Skip other comments
        if kind == "line_comment" || kind == "block_comment" {
            continue;
        }
        // Skip identifiers that are struct names (first child of struct)
        // We only want to consider fields and values as trailing anchors
        if kind == "identifier" {
            // Check if this identifier is the first named child (struct name)
            // by seeing if there are any fields/values before it
            let has_field_before = siblings_before.iter().any(|s| {
                s.kind() == "field" || s.kind() == "map_entry"
            });
            if !has_field_before {
                // This is likely the struct name, skip it
                continue;
            }
        }
        // This is a significant node - check if it ends on or after the line start
        if sibling.end_byte() > line_start {
            return true;
        }
        // If this sibling is on a previous line, stop looking
        break;
    }

    false
}

/// Collect top-level comments (direct children of source_file that come before the main value)
fn collect_top_level_comments(root: &Node, content: &str, main_value: &Node) -> Vec<Comment> {
    let mut comments = Vec::new();
    let mut cursor = root.walk();

    for child in root.children(&mut cursor) {
        let kind = child.kind();
        if kind == "line_comment" || kind == "block_comment" {
            // Only include comments that come before the main value
            if child.end_byte() <= main_value.start_byte() {
                if let Some(text) = ts_utils::node_text(&child, content) {
                    comments.push(Comment {
                        text: text.to_string(),
                        start_byte: child.start_byte(),
                        end_byte: child.end_byte(),
                        is_trailing: false,
                    });
                }
            }
        }
    }

    comments.sort_by_key(|c| c.start_byte);
    comments
}

/// Format RON content using tree-sitter AST, preserving comments
pub fn format_ron(content: &str) -> String {
    let indent_str = "    "; // 4 spaces

    let mut parser = RonParser::new();
    let tree = match parser.parse(content) {
        Some(t) => t,
        None => {
            // If parsing fails, return original content
            return content.to_string();
        }
    };

    // Build the formatted output
    let mut result = String::new();

    // Format the main value
    if let Some(main_value) = ts_utils::find_main_value(&tree) {
        // Collect and emit top-level comments
        let top_comments = collect_top_level_comments(&tree.root_node(), content, &main_value);
        for comment in &top_comments {
            result.push_str(&comment.text);
            result.push('\n');
        }

        // Track which comments we've emitted to avoid duplicates
        let mut emitted_comments = HashSet::new();
        for comment in &top_comments {
            emitted_comments.insert(comment.start_byte);
        }

        format_node(
            &main_value,
            content,
            &mut result,
            0,
            indent_str,
            false,
            &mut emitted_comments,
        );
    }

    result.trim_end().to_string()
}

/// Format a single node recursively
fn format_node(
    node: &Node,
    content: &str,
    output: &mut String,
    indent_level: usize,
    indent_str: &str,
    inline: bool,
    emitted: &mut HashSet<usize>,
) {
    let kind = node.kind();

    match kind {
        "struct" => format_struct(node, content, output, indent_level, indent_str, inline, emitted),
        "array" => format_array(node, content, output, indent_level, indent_str, inline, emitted),
        "map" => format_map(node, content, output, indent_level, indent_str, inline, emitted),
        "tuple" => format_tuple(node, content, output, indent_level, indent_str, inline, emitted),
        "field" => format_field(node, content, output, indent_level, indent_str, emitted),
        "string" | "integer" | "float" | "boolean" | "char" | "identifier" | "unit" => {
            // Leaf nodes - just output their text
            if let Some(text) = ts_utils::node_text(node, content) {
                output.push_str(text);
            }
        }
        _ => {
            // For other nodes, just output their text as-is
            if let Some(text) = ts_utils::node_text(node, content) {
                output.push_str(text);
            }
        }
    }
}

/// Format a struct node
fn format_struct(
    node: &Node,
    content: &str,
    output: &mut String,
    indent_level: usize,
    indent_str: &str,
    _inline: bool,
    emitted: &mut HashSet<usize>,
) {
    // Get struct name if it exists
    if let Some(name) = ts_utils::struct_name(node, content) {
        output.push_str(name);
    }

    // Check if empty
    let is_empty = ts_utils::is_empty_structure(node, content);

    if is_empty {
        output.push_str("()");
        return;
    }

    output.push('(');

    // Get all fields and values to determine if this is a tuple-style or field-style struct
    let fields = ts_utils::struct_fields(node);
    let values = ts_utils::struct_values(node, content);

    // Collect comments that are direct children of this struct
    let inner_comments = collect_inner_comments(node, content);

    // If we have fields, use field formatting
    if !fields.is_empty() {
        output.push('\n');

        for (i, field) in fields.iter().enumerate() {
            // Emit any leading comments that come before this field
            for comment in &inner_comments {
                if emitted.contains(&comment.start_byte) {
                    continue;
                }
                if !comment.is_trailing
                    && comment.end_byte <= field.start_byte()
                    && (i == 0 || comment.start_byte > fields[i - 1].end_byte())
                {
                    output.push_str(&indent_str.repeat(indent_level + 1));
                    output.push_str(&comment.text);
                    output.push('\n');
                    emitted.insert(comment.start_byte);
                }
            }

            output.push_str(&indent_str.repeat(indent_level + 1));
            format_field(field, content, output, indent_level + 1, indent_str, emitted);

            // Add comma after each field
            output.push(',');

            // Emit trailing comments for this field
            for comment in &inner_comments {
                if emitted.contains(&comment.start_byte) {
                    continue;
                }
                if comment.is_trailing && comment.start_byte > field.end_byte() {
                    // Check it's on the same "logical" position (before next field or end)
                    let next_field_start = fields.get(i + 1).map(|f| f.start_byte());
                    let is_before_next = next_field_start
                        .map(|s| comment.end_byte <= s)
                        .unwrap_or(true);
                    if is_before_next {
                        output.push(' ');
                        output.push_str(&comment.text);
                        emitted.insert(comment.start_byte);
                        break; // Only one trailing comment per field
                    }
                }
            }

            output.push('\n');
        }

        // Emit any remaining comments after the last field (non-trailing)
        if let Some(last_field) = fields.last() {
            for comment in &inner_comments {
                if emitted.contains(&comment.start_byte) {
                    continue;
                }
                if comment.start_byte > last_field.end_byte() && !comment.is_trailing {
                    output.push_str(&indent_str.repeat(indent_level + 1));
                    output.push_str(&comment.text);
                    output.push('\n');
                    emitted.insert(comment.start_byte);
                }
            }
        }

        output.push_str(&indent_str.repeat(indent_level));
    } else if !values.is_empty() {
        // Tuple-style struct like Some("value") - these should stay inline if single element
        let should_inline = values.len() == 1;

        if !should_inline {
            output.push('\n');
        }

        for (i, child) in values.iter().enumerate() {
            if !should_inline {
                // Emit leading comments
                for comment in &inner_comments {
                    if emitted.contains(&comment.start_byte) {
                        continue;
                    }
                    if !comment.is_trailing
                        && comment.end_byte <= child.start_byte()
                        && (i == 0 || comment.start_byte > values[i - 1].end_byte())
                    {
                        output.push_str(&indent_str.repeat(indent_level + 1));
                        output.push_str(&comment.text);
                        output.push('\n');
                        emitted.insert(comment.start_byte);
                    }
                }
                output.push_str(&indent_str.repeat(indent_level + 1));
            }
            format_node(
                child,
                content,
                output,
                indent_level + 1,
                indent_str,
                should_inline,
                emitted,
            );

            if i < values.len() - 1 {
                output.push(',');
                if !should_inline {
                    output.push('\n');
                } else {
                    output.push(' ');
                }
            }
        }

        if !should_inline {
            output.push('\n');
            output.push_str(&indent_str.repeat(indent_level));
        }
    }

    output.push(')');
}

/// Format a field node
fn format_field(
    node: &Node,
    content: &str,
    output: &mut String,
    indent_level: usize,
    indent_str: &str,
    emitted: &mut HashSet<usize>,
) {
    // Get field name
    if let Some(name) = ts_utils::field_name(node, content) {
        output.push_str(name);
        output.push_str(": ");
    }

    // Get field value
    if let Some(value) = ts_utils::field_value(node) {
        format_node(&value, content, output, indent_level, indent_str, false, emitted);
    }
}

/// Format an array node
fn format_array(
    node: &Node,
    content: &str,
    output: &mut String,
    indent_level: usize,
    indent_str: &str,
    _inline: bool,
    emitted: &mut HashSet<usize>,
) {
    let is_empty = ts_utils::is_empty_structure(node, content);

    if is_empty {
        output.push_str("[]");
        return;
    }

    output.push('[');

    // Get all array elements (named children that aren't comments)
    let elements: Vec<_> = ts_utils::named_children(node)
        .into_iter()
        .filter(|n| n.kind() != "line_comment" && n.kind() != "block_comment")
        .collect();

    let inner_comments = collect_inner_comments(node, content);

    if !elements.is_empty() {
        output.push('\n');

        for (i, element) in elements.iter().enumerate() {
            // Emit leading comments before this element
            for comment in &inner_comments {
                if emitted.contains(&comment.start_byte) {
                    continue;
                }
                if !comment.is_trailing
                    && comment.end_byte <= element.start_byte()
                    && (i == 0 || comment.start_byte > elements[i - 1].end_byte())
                {
                    output.push_str(&indent_str.repeat(indent_level + 1));
                    output.push_str(&comment.text);
                    output.push('\n');
                    emitted.insert(comment.start_byte);
                }
            }

            output.push_str(&indent_str.repeat(indent_level + 1));
            format_node(
                element,
                content,
                output,
                indent_level + 1,
                indent_str,
                false,
                emitted,
            );

            output.push(',');

            // Emit trailing comments for this element
            for comment in &inner_comments {
                if emitted.contains(&comment.start_byte) {
                    continue;
                }
                if comment.is_trailing && comment.start_byte > element.end_byte() {
                    let next_elem_start = elements.get(i + 1).map(|e| e.start_byte());
                    let is_before_next = next_elem_start
                        .map(|s| comment.end_byte <= s)
                        .unwrap_or(true);
                    if is_before_next {
                        output.push(' ');
                        output.push_str(&comment.text);
                        emitted.insert(comment.start_byte);
                        break;
                    }
                }
            }

            output.push('\n');
        }

        // Emit any remaining comments after the last element
        if let Some(last) = elements.last() {
            for comment in &inner_comments {
                if emitted.contains(&comment.start_byte) {
                    continue;
                }
                if comment.start_byte > last.end_byte() && !comment.is_trailing {
                    output.push_str(&indent_str.repeat(indent_level + 1));
                    output.push_str(&comment.text);
                    output.push('\n');
                    emitted.insert(comment.start_byte);
                }
            }
        }

        output.push_str(&indent_str.repeat(indent_level));
    }

    output.push(']');
}

/// Format a map node
fn format_map(
    node: &Node,
    content: &str,
    output: &mut String,
    indent_level: usize,
    indent_str: &str,
    _inline: bool,
    emitted: &mut HashSet<usize>,
) {
    let is_empty = ts_utils::is_empty_structure(node, content);

    if is_empty {
        output.push_str("{}");
        return;
    }

    output.push('{');

    // Get all map entries
    let entries = ts_utils::children_by_kind(node, "map_entry");
    let inner_comments = collect_inner_comments(node, content);

    if !entries.is_empty() {
        output.push('\n');

        for (i, entry) in entries.iter().enumerate() {
            // Emit leading comments before this entry
            for comment in &inner_comments {
                if emitted.contains(&comment.start_byte) {
                    continue;
                }
                if !comment.is_trailing
                    && comment.end_byte <= entry.start_byte()
                    && (i == 0 || comment.start_byte > entries[i - 1].end_byte())
                {
                    output.push_str(&indent_str.repeat(indent_level + 1));
                    output.push_str(&comment.text);
                    output.push('\n');
                    emitted.insert(comment.start_byte);
                }
            }

            output.push_str(&indent_str.repeat(indent_level + 1));

            // Format map entry (key: value)
            let children: Vec<_> = ts_utils::named_children(entry)
                .into_iter()
                .filter(|n| n.kind() != "line_comment" && n.kind() != "block_comment")
                .collect();

            if children.len() >= 2 {
                // Key
                format_node(
                    &children[0],
                    content,
                    output,
                    indent_level + 1,
                    indent_str,
                    false,
                    emitted,
                );
                output.push_str(": ");
                // Value
                format_node(
                    &children[1],
                    content,
                    output,
                    indent_level + 1,
                    indent_str,
                    false,
                    emitted,
                );
            }

            output.push(',');

            // Emit trailing comments for this entry
            for comment in &inner_comments {
                if emitted.contains(&comment.start_byte) {
                    continue;
                }
                if comment.is_trailing && comment.start_byte > entry.end_byte() {
                    let next_entry_start = entries.get(i + 1).map(|e| e.start_byte());
                    let is_before_next = next_entry_start
                        .map(|s| comment.end_byte <= s)
                        .unwrap_or(true);
                    if is_before_next {
                        output.push(' ');
                        output.push_str(&comment.text);
                        emitted.insert(comment.start_byte);
                        break;
                    }
                }
            }

            output.push('\n');
        }

        // Emit any remaining comments after the last entry
        if let Some(last) = entries.last() {
            for comment in &inner_comments {
                if emitted.contains(&comment.start_byte) {
                    continue;
                }
                if comment.start_byte > last.end_byte() && !comment.is_trailing {
                    output.push_str(&indent_str.repeat(indent_level + 1));
                    output.push_str(&comment.text);
                    output.push('\n');
                    emitted.insert(comment.start_byte);
                }
            }
        }

        output.push_str(&indent_str.repeat(indent_level));
    }

    output.push('}');
}

/// Format a tuple node
fn format_tuple(
    node: &Node,
    content: &str,
    output: &mut String,
    indent_level: usize,
    indent_str: &str,
    _inline: bool,
    emitted: &mut HashSet<usize>,
) {
    output.push('(');

    let elements: Vec<_> = ts_utils::named_children(node)
        .into_iter()
        .filter(|n| n.kind() != "line_comment" && n.kind() != "block_comment")
        .collect();

    let inner_comments = collect_inner_comments(node, content);

    if !elements.is_empty() {
        output.push('\n');

        for (i, element) in elements.iter().enumerate() {
            // Emit leading comments before this element
            for comment in &inner_comments {
                if emitted.contains(&comment.start_byte) {
                    continue;
                }
                if !comment.is_trailing
                    && comment.end_byte <= element.start_byte()
                    && (i == 0 || comment.start_byte > elements[i - 1].end_byte())
                {
                    output.push_str(&indent_str.repeat(indent_level + 1));
                    output.push_str(&comment.text);
                    output.push('\n');
                    emitted.insert(comment.start_byte);
                }
            }

            output.push_str(&indent_str.repeat(indent_level + 1));
            format_node(
                element,
                content,
                output,
                indent_level + 1,
                indent_str,
                false,
                emitted,
            );

            output.push(',');

            // Emit trailing comments for this element
            for comment in &inner_comments {
                if emitted.contains(&comment.start_byte) {
                    continue;
                }
                if comment.is_trailing && comment.start_byte > element.end_byte() {
                    let next_elem_start = elements.get(i + 1).map(|e| e.start_byte());
                    let is_before_next = next_elem_start
                        .map(|s| comment.end_byte <= s)
                        .unwrap_or(true);
                    if is_before_next {
                        output.push(' ');
                        output.push_str(&comment.text);
                        emitted.insert(comment.start_byte);
                        break;
                    }
                }
            }

            output.push('\n');
        }

        // Emit any remaining comments after the last element
        if let Some(last) = elements.last() {
            for comment in &inner_comments {
                if emitted.contains(&comment.start_byte) {
                    continue;
                }
                if comment.start_byte > last.end_byte() && !comment.is_trailing {
                    output.push_str(&indent_str.repeat(indent_level + 1));
                    output.push_str(&comment.text);
                    output.push('\n');
                    emitted.insert(comment.start_byte);
                }
            }
        }

        output.push_str(&indent_str.repeat(indent_level));
    }

    output.push(')');
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simple_struct() {
        let input = "User(id: 1, name: \"Alice\")";
        let formatted = format_ron(input);
        println!("Formatted:\n{}", formatted);
        assert!(formatted.contains("User("));
        assert!(formatted.contains("    id: 1,"));
        assert!(formatted.contains("    name: \"Alice\","));
        assert!(formatted.contains(")"));
    }

    #[test]
    fn test_empty_parens() {
        let input = "Unit()";
        let formatted = format_ron(input);
        println!("Formatted: '{}'", formatted);
        assert_eq!(formatted, "Unit()");
    }

    #[test]
    fn test_nested_struct() {
        let input = "Post(author: User(id: 1))";
        let formatted = format_ron(input);
        println!("Formatted:\n{}", formatted);
        assert!(formatted.contains("Post("));
        assert!(formatted.contains("    author: User("));
        assert!(formatted.contains("        id: 1,"));
        assert!(formatted.contains("    )"));
        assert!(formatted.trim().ends_with(")"));
    }

    #[test]
    fn test_array() {
        let input = r#"Config(roles: ["admin", "user"])"#;
        let formatted = format_ron(input);
        println!("Formatted:\n{}", formatted);
        assert!(formatted.contains("roles: ["));
        assert!(formatted.contains(r#"        "admin","#));
        assert!(formatted.contains(r#"        "user","#));
        assert!(formatted.contains("    ],") || formatted.contains("    ]\n)"));
    }

    // Comment preservation tests

    #[test]
    fn test_top_level_line_comment() {
        let input = "// This is a config file\nUser(id: 1)";
        let formatted = format_ron(input);
        println!("Formatted:\n{}", formatted);
        assert!(formatted.contains("// This is a config file"));
        assert!(formatted.contains("User("));
        assert!(formatted.contains("    id: 1,"));
    }

    #[test]
    fn test_top_level_block_comment() {
        let input = "/* Config for user */\nUser(id: 1)";
        let formatted = format_ron(input);
        println!("Formatted:\n{}", formatted);
        assert!(formatted.contains("/* Config for user */"));
        assert!(formatted.contains("User("));
    }

    #[test]
    fn test_comment_before_field() {
        let input = "User(\n    // The user's ID\n    id: 1,\n)";
        let formatted = format_ron(input);
        println!("Formatted:\n{}", formatted);
        assert!(formatted.contains("    // The user's ID"));
        assert!(formatted.contains("    id: 1,"));
    }

    #[test]
    fn test_inline_comment_after_field() {
        let input = "User(\n    id: 1, // User identifier\n)";
        let formatted = format_ron(input);
        println!("Formatted:\n{}", formatted);
        assert!(formatted.contains("id: 1, // User identifier"));
    }

    #[test]
    fn test_multiple_comments() {
        let input = r#"// Top comment
User(
    // Comment for id
    id: 1, // ID value
    // Comment for name
    name: "Alice", // Name value
)"#;
        let formatted = format_ron(input);
        println!("Formatted:\n{}", formatted);
        assert!(formatted.contains("// Top comment"));
        assert!(formatted.contains("    // Comment for id"));
        assert!(formatted.contains("id: 1, // ID value"));
        assert!(formatted.contains("    // Comment for name"));
        assert!(formatted.contains("name: \"Alice\", // Name value"));
    }

    #[test]
    fn test_comment_in_array() {
        let input = r#"Config(
    roles: [
        // Admin role
        "admin",
        // User role
        "user",
    ],
)"#;
        let formatted = format_ron(input);
        println!("Formatted:\n{}", formatted);
        assert!(formatted.contains("        // Admin role"));
        assert!(formatted.contains("        \"admin\","));
        assert!(formatted.contains("        // User role"));
        assert!(formatted.contains("        \"user\","));
    }

    #[test]
    fn test_block_comment_inside_struct() {
        let input = "User(/* user id */ id: 1)";
        let formatted = format_ron(input);
        println!("Formatted:\n{}", formatted);
        assert!(formatted.contains("/* user id */"));
        assert!(formatted.contains("id: 1,"));
    }

    #[test]
    fn test_nested_struct_with_comments() {
        let input = r#"// Post configuration
Post(
    // Author information
    author: User(
        // The author's ID
        id: 1,
    ),
)"#;
        let formatted = format_ron(input);
        println!("Formatted:\n{}", formatted);
        assert!(formatted.contains("// Post configuration"));
        assert!(formatted.contains("    // Author information"));
        assert!(formatted.contains("        // The author's ID"));
    }
}
