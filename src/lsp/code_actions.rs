use super::tree_sitter_parser;
use crate::rust_analyzer::{FieldInfo, RustAnalyzer, TypeInfo, TypeKind};
use std::sync::Arc;
use tower_lsp::lsp_types::*;
use tree_sitter::Tree;

/// Generate all code actions for a RON document
pub fn generate_code_actions(
    tree: &Tree,
    content: &str,
    type_info: &TypeInfo,
    url: &Url,
    analyzer: Arc<RustAnalyzer>,
    context_diagnostics: &[Diagnostic],
) -> Vec<CodeActionOrCommand> {
    let mut actions = Vec::new();

    // Quick-fixes for diagnostics in the requested range (remove unknown/duplicate fields)
    actions.extend(generate_remove_field_actions(
        tree,
        content,
        context_diagnostics,
        url,
    ));

    // Add actions for making struct names explicit
    actions.extend(generate_explicit_type_actions(tree, content, type_info, url));

    // Add actions for missing fields (handles both structs and enum variants)
    actions.extend(generate_missing_field_actions(
        tree, content, type_info, url, &analyzer,
    ));

    // Also check for nested enum variant fields
    actions.extend(generate_missing_variant_field_actions(
        tree, content, type_info, url, &analyzer,
    ));

    actions
}

/// Quick-fixes that delete fields flagged as `unknown-field` or
/// `duplicate-field` by the published diagnostics.
pub fn generate_remove_field_actions(
    tree: &Tree,
    content: &str,
    diagnostics: &[Diagnostic],
    url: &Url,
) -> Vec<CodeActionOrCommand> {
    use super::diagnostics::codes;
    use super::ts_utils;

    let mut actions = Vec::new();
    let lines: Vec<&str> = content.lines().collect();

    for diag in diagnostics {
        let removable = matches!(
            &diag.code,
            Some(NumberOrString::String(c))
                if c == codes::UNKNOWN_FIELD || c == codes::DUPLICATE_FIELD
        );
        if !removable {
            continue;
        }

        let offset = ts_utils::position_to_byte_offset(content, diag.range.start);
        let Some(node) = tree.root_node().descendant_for_byte_range(offset, offset) else {
            continue;
        };
        let Some(field_node) = (if node.kind() == "field" {
            Some(node)
        } else {
            ts_utils::find_ancestor_by_kind(node, "field")
        }) else {
            continue;
        };
        let Some(name) = ts_utils::field_name(&field_node, content) else {
            continue;
        };

        // Delete through the trailing comma if present
        let start = field_node.start_position();
        let mut end = field_node.end_position();
        if let Some(next) = field_node.next_sibling()
            && next.kind() == ","
        {
            end = next.end_position();
        }

        // If the field occupies its line(s) alone, remove the whole lines
        let line_prefix = lines
            .get(start.row)
            .map(|l| &l[..start.column.min(l.len())])
            .unwrap_or("");
        let line_suffix = lines
            .get(end.row)
            .map(|l| &l[end.column.min(l.len())..])
            .unwrap_or("");
        let range = if line_prefix.trim().is_empty() && line_suffix.trim().is_empty() {
            Range::new(
                Position::new(start.row as u32, 0),
                Position::new(end.row as u32 + 1, 0),
            )
        } else {
            Range::new(
                Position::new(start.row as u32, start.column as u32),
                Position::new(end.row as u32, end.column as u32),
            )
        };

        let mut changes = std::collections::HashMap::new();
        changes.insert(
            url.clone(),
            vec![TextEdit {
                range,
                new_text: String::new(),
            }],
        );

        actions.push(CodeActionOrCommand::CodeAction(CodeAction {
            title: format!("Remove field '{}'", name),
            kind: Some(CodeActionKind::QUICKFIX),
            diagnostics: Some(vec![diag.clone()]),
            edit: Some(WorkspaceEdit {
                changes: Some(changes),
                ..Default::default()
            }),
            ..Default::default()
        }));
    }

    actions
}

/// Generate code actions for adding missing fields in nested enum variants
fn generate_missing_variant_field_actions(
    tree: &Tree,
    content: &str,
    type_info: &TypeInfo,
    url: &Url,
    analyzer: &RustAnalyzer,
) -> Vec<CodeActionOrCommand> {
    let mut actions = Vec::new();
    let variant_locations = tree_sitter_parser::find_all_variant_field_locations(tree, content);

    // Group locations by (containing_field_name, variant_name) to find which fields are used
    let mut variant_fields_map: std::collections::HashMap<
        (String, String),
        std::collections::HashSet<String>,
    > = std::collections::HashMap::new();

    // Collect all fields used for each variant
    for location in &variant_locations {
        if let Some(ref field_at_pos) = location.field_at_position {
            let key = (
                location.containing_field_name.clone(),
                location.variant_name.clone(),
            );
            variant_fields_map
                .entry(key)
                .or_default()
                .insert(field_at_pos.clone());
        }
    }

    // Now generate actions for each unique variant
    let mut seen_variants = std::collections::HashSet::new();
    for location in variant_locations {
        let key = (
            location.containing_field_name.clone(),
            location.variant_name.clone(),
        );
        if seen_variants.contains(&key) {
            continue;
        }
        seen_variants.insert(key.clone());

        // Resolve the containing field directly on the root type; deeper nesting
        // is not currently handled here
        let Some(field) = type_info.find_field_serialized(&location.containing_field_name) else {
            continue;
        };

        // Get the enum type for this field and find the variant definition
        let Some(field_type_info) = analyzer.get_type_info(&field.type_name) else {
            continue;
        };
        let Some(variant) = field_type_info.find_variant_serialized(&location.variant_name) else {
            continue;
        };

        // Get fields used in this variant from our map
        let used_fields = variant_fields_map.get(&key).cloned().unwrap_or_default();

        let required_missing = if field_type_info.has_default {
            Vec::new()
        } else {
            super::type_utils::missing_required_fields(&variant.effective_fields(), |name| {
                used_fields.contains(name)
            })
        };

        if !required_missing.is_empty()
            && let Some(edit) = generate_field_insertions(tree, &required_missing)
        {
            let mut changes = std::collections::HashMap::new();
            changes.insert(url.clone(), vec![edit]);

            actions.push(CodeActionOrCommand::CodeAction(CodeAction {
                title: format!(
                    "Add {} required field{} to {}::{}",
                    required_missing.len(),
                    if required_missing.len() == 1 { "" } else { "s" },
                    location.containing_field_name,
                    location.variant_name
                ),
                kind: Some(CodeActionKind::QUICKFIX),
                edit: Some(WorkspaceEdit {
                    changes: Some(changes),
                    ..Default::default()
                }),
                ..Default::default()
            }));
        }
    }

    actions
}

/// Generate code actions for making implicit struct names explicit
fn generate_explicit_type_actions(
    tree: &Tree,
    content: &str,
    type_info: &TypeInfo,
    url: &Url,
) -> Vec<CodeActionOrCommand> {
    let mut actions = Vec::new();

    // Check if the root level uses unnamed struct syntax
    if let Some(action) = create_explicit_root_type_action(tree, content, type_info, url) {
        actions.push(action);
    }

    // Check for nested unnamed structs in field values
    if let Some(fields) = type_info.fields() {
        let rename_all = type_info.rename_all.as_deref();
        for field in fields {
            if let Some(action) =
                create_explicit_field_type_action(tree, content, field, rename_all, url)
            {
                actions.push(action);
            }
        }
    }

    actions
}

/// Generate code actions for adding missing fields
fn generate_missing_field_actions(
    tree: &Tree,
    content: &str,
    type_info: &TypeInfo,
    url: &Url,
    analyzer: &RustAnalyzer,
) -> Vec<CodeActionOrCommand> {
    let mut actions = Vec::new();

    // Check if we're in an enum variant context
    if matches!(type_info.kind, TypeKind::Enum(_)) {
        // Try to detect which variant we're in
        if let Some(variant_name) = detect_current_variant_in_content(content)
            && let Some(variant) = type_info.find_variant_serialized(&variant_name)
        {
            // Generate actions for this variant's fields
            let ron_fields = tree_sitter_parser::extract_fields_from_ron(tree, content);
            let effective_fields = variant.effective_fields();
            let all_missing: Vec<_> = effective_fields
                .iter()
                .filter(|(name, field)| {
                    !ron_fields.contains(name) && !ron_fields.contains(&field.name)
                })
                .cloned()
                .collect();

            let required_missing = if type_info.has_default {
                Vec::new()
            } else {
                super::type_utils::missing_required_fields(&effective_fields, |name| {
                    ron_fields.iter().any(|f| f == name)
                })
            };

            // Code action: Add all required fields for variant
            if !required_missing.is_empty()
                && let Some(edit) = generate_field_insertions(tree, &required_missing)
            {
                let mut changes = std::collections::HashMap::new();
                changes.insert(url.clone(), vec![edit]);

                actions.push(CodeActionOrCommand::CodeAction(CodeAction {
                    title: format!(
                        "Add {} required field{} to {}",
                        required_missing.len(),
                        if required_missing.len() == 1 { "" } else { "s" },
                        variant_name
                    ),
                    kind: Some(CodeActionKind::QUICKFIX),
                    edit: Some(WorkspaceEdit {
                        changes: Some(changes),
                        ..Default::default()
                    }),
                    ..Default::default()
                }));
            }

            // Code action: Add all fields for variant
            if !all_missing.is_empty()
                && let Some(edit) = generate_field_insertions(tree, &all_missing)
            {
                let mut changes = std::collections::HashMap::new();
                changes.insert(url.clone(), vec![edit]);

                actions.push(CodeActionOrCommand::CodeAction(CodeAction {
                    title: format!(
                        "Add all {} missing field{} to {}",
                        all_missing.len(),
                        if all_missing.len() == 1 { "" } else { "s" },
                        variant_name
                    ),
                    kind: Some(CodeActionKind::QUICKFIX),
                    edit: Some(WorkspaceEdit {
                        changes: Some(changes),
                        ..Default::default()
                    }),
                    ..Default::default()
                }));
            }

            return actions;
        }
    }

    // Original struct logic
    let ron_fields = tree_sitter_parser::extract_fields_from_ron(tree, content);
    let effective_fields = type_info.effective_fields(analyzer);

    // Find missing fields
    let all_missing: Vec<_> = effective_fields
        .iter()
        .filter(|(name, field)| !ron_fields.contains(name) && !ron_fields.contains(&field.name))
        .cloned()
        .collect();

    // Find required missing fields (not Option<T> and no Default trait)
    let required_missing = if type_info.has_default {
        Vec::new()
    } else {
        super::type_utils::missing_required_fields(&effective_fields, |name| {
            ron_fields.iter().any(|f| f == name)
        })
    };

    // Code action: Add all required fields
    if !required_missing.is_empty()
        && let Some(edit) = generate_field_insertions(tree, &required_missing)
    {
        let mut changes = std::collections::HashMap::new();
        changes.insert(url.clone(), vec![edit]);

        actions.push(CodeActionOrCommand::CodeAction(CodeAction {
            title: format!(
                "Add {} required field{}",
                required_missing.len(),
                if required_missing.len() == 1 { "" } else { "s" }
            ),
            kind: Some(CodeActionKind::QUICKFIX),
            edit: Some(WorkspaceEdit {
                changes: Some(changes),
                ..Default::default()
            }),
            ..Default::default()
        }));
    }

    // Code action: Add all fields
    if !all_missing.is_empty()
        && let Some(edit) = generate_field_insertions(tree, &all_missing)
    {
        let mut changes = std::collections::HashMap::new();
        changes.insert(url.clone(), vec![edit]);

        actions.push(CodeActionOrCommand::CodeAction(CodeAction {
            title: format!(
                "Add all {} missing field{}",
                all_missing.len(),
                if all_missing.len() == 1 { "" } else { "s" }
            ),
            kind: Some(CodeActionKind::QUICKFIX),
            edit: Some(WorkspaceEdit {
                changes: Some(changes),
                ..Default::default()
            }),
            ..Default::default()
        }));
    }

    actions
}

/// Create action to make root-level struct name explicit using tree-sitter
/// Converts: `(field: value)` → `StructName(field: value)`
fn create_explicit_root_type_action(
    tree: &Tree,
    content: &str,
    type_info: &TypeInfo,
    url: &Url,
) -> Option<CodeActionOrCommand> {
    use super::ts_utils;

    let main_value = ts_utils::find_main_value(tree)?;

    if main_value.kind() == "struct" && ts_utils::struct_name(&main_value, content).is_none() {
        let type_name = super::type_utils::short_name(&type_info.name);
        let pos = main_value.start_position();

        let mut changes = std::collections::HashMap::new();
        changes.insert(
            url.clone(),
            vec![TextEdit {
                range: Range::new(
                    Position::new(pos.row as u32, pos.column as u32),
                    Position::new(pos.row as u32, pos.column as u32),
                ),
                new_text: type_name.to_string(),
            }],
        );

        return Some(CodeActionOrCommand::CodeAction(CodeAction {
            title: format!("Make struct name explicit: {}", type_name),
            kind: Some(CodeActionKind::REFACTOR),
            edit: Some(WorkspaceEdit {
                changes: Some(changes),
                ..Default::default()
            }),
            ..Default::default()
        }));
    }

    None
}

/// Create action to make nested field type explicit using tree-sitter
/// Converts: `foo: (value)` → `foo: TypeName(value)`
fn create_explicit_field_type_action(
    tree: &Tree,
    content: &str,
    field: &FieldInfo,
    rename_all: Option<&str>,
    url: &Url,
) -> Option<CodeActionOrCommand> {
    use super::ts_utils;

    let main_value = ts_utils::find_main_value(tree)?;

    if main_value.kind() == "struct" {
        let field_nodes = ts_utils::struct_fields(&main_value);
        let serialized = field.serialized_name(rename_all);

        for field_node in field_nodes {
            if let Some(field_name) = ts_utils::field_name(&field_node, content)
                && (field_name == serialized || field_name == field.name)
                && let Some(value_node) = ts_utils::field_value(&field_node)
                && value_node.kind() == "struct"
                && ts_utils::struct_name(&value_node, content).is_none()
            {
                let type_name = field
                    .type_name
                    .split("::")
                    .last()
                    .unwrap_or(&field.type_name)
                    .replace(" ", "");
                let clean_type = if type_name.starts_with("Option<") && type_name.ends_with('>') {
                    &type_name[7..type_name.len() - 1]
                } else {
                    &type_name
                };

                let pos = value_node.start_position();
                let mut changes = std::collections::HashMap::new();
                changes.insert(
                    url.clone(),
                    vec![TextEdit {
                        range: Range::new(
                            Position::new(pos.row as u32, pos.column as u32),
                            Position::new(pos.row as u32, pos.column as u32),
                        ),
                        new_text: clean_type.to_string(),
                    }],
                );

                return Some(CodeActionOrCommand::CodeAction(CodeAction {
                    title: format!("Make field type explicit: {} {}", field.name, clean_type),
                    kind: Some(CodeActionKind::REFACTOR),
                    edit: Some(WorkspaceEdit {
                        changes: Some(changes),
                        ..Default::default()
                    }),
                    ..Default::default()
                }));
            }
        }
    }

    None
}

/// Generate text edits to insert missing fields using tree-sitter.
/// Takes `(serialized_name, field)` pairs so inserted names match what serde expects.
fn generate_field_insertions(
    tree: &Tree,
    missing_fields: &[(String, FieldInfo)],
) -> Option<TextEdit> {
    use super::ts_utils;

    let root = tree.root_node();

    // Try to find struct node even if there's an ERROR (for :: syntax)
    let main_value = match ts_utils::find_main_value(tree) {
        Some(v) => v,
        None => {
            // Fallback: look for any struct node if main value is ERROR
            let mut cursor = root.walk();
            let result = root.children(&mut cursor).find(|n| n.kind() == "struct");
            match result {
                Some(s) => s,
                None => {
                    return None;
                }
            }
        }
    };

    if main_value.kind() != "struct" && main_value.kind() != "ERROR" {
        return None;
    }

    // For ERROR nodes, the struct is likely a SIBLING, not a child
    let struct_node = if main_value.kind() == "ERROR" {
        // Look for struct node among root's children
        let mut cursor = root.walk();
        let result = root.children(&mut cursor).find(|n| n.kind() == "struct");
        match result {
            Some(s) => s,
            None => {
                return None;
            }
        }
    } else {
        main_value
    };

    // Find the closing paren position
    let end_pos = struct_node.end_position();
    let insert_line = end_pos.row as u32;
    let insert_col = end_pos.column.saturating_sub(1) as u32; // Before the closing paren

    // Check if we have existing fields to determine if we need a comma
    let existing_fields = ts_utils::struct_fields(&struct_node);
    let needs_comma = !existing_fields.is_empty();

    // Generate the field text
    let mut field_text = String::new();
    if needs_comma {
        field_text.push_str(",\n");
    } else if insert_line > 0 {
        field_text.push('\n');
    }

    // Use default 4-space indentation
    let indent = "    ";

    for (i, (name, field)) in missing_fields.iter().enumerate() {
        field_text.push_str(indent);
        field_text.push_str(name);
        field_text.push_str(": ");
        field_text.push_str(&generate_default_value(&field.type_name));
        if i < missing_fields.len() - 1 {
            field_text.push(',');
        }
        field_text.push('\n');
    }

    Some(TextEdit {
        range: Range::new(
            Position::new(insert_line, insert_col),
            Position::new(insert_line, insert_col),
        ),
        new_text: field_text,
    })
}

/// Detect which enum variant we're currently inside based on the content
/// This primarily looks for EnumName::VariantName( pattern
/// Returns None for regular structs without :: prefix
fn detect_current_variant_in_content(content: &str) -> Option<String> {
    // Look for :: pattern which indicates it's definitely a variant
    let double_colon_idx = content.find("::")?;

    // Find the variant name after ::
    let after_colons = &content[double_colon_idx + 2..];
    let variant_start = after_colons
        .chars()
        .position(|c| c.is_alphanumeric() || c == '_')?;
    let variant_part = &after_colons[variant_start..];

    // Extract the variant name (alphanumeric + underscore until opening paren/brace or whitespace)
    let variant_end = variant_part
        .chars()
        .position(|c| !c.is_alphanumeric() && c != '_')
        .unwrap_or(variant_part.len());

    let variant_name = &variant_part[..variant_end];

    // Check that there's an opening paren or brace after the variant name (with optional whitespace)
    let remaining = variant_part[variant_end..].trim_start();
    if (remaining.starts_with('(') || remaining.starts_with('{')) && !variant_name.is_empty() {
        return Some(variant_name.to_string());
    }

    None
}

/// Generate a default value for a given Rust type
fn generate_default_value(type_name: &str) -> String {
    let clean = type_name.replace(" ", "");

    if clean.starts_with("Option") {
        "None".to_string()
    } else if clean == "bool" {
        "false".to_string()
    } else if clean.starts_with("Vec") || clean.starts_with("[") {
        "[]".to_string()
    } else if clean.starts_with("HashMap") || clean.starts_with("BTreeMap") {
        "{}".to_string()
    } else if clean == "String" || clean == "&str" || clean == "str" {
        "\"\"".to_string()
    } else if clean
        .chars()
        .all(|c| c.is_numeric() || c == 'i' || c == 'u' || c == 'f')
    {
        // Numeric types
        "0".to_string()
    } else {
        // Custom type - use constructor notation with placeholder
        format!("{}()", clean)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(content: &str) -> Tree {
        super::super::ts_utils::RonParser::new().parse(content).unwrap()
    }
    use crate::rust_analyzer::TypeKind;

    #[test]
    fn test_explicit_root_type_same_line() {
        let content = "(id: 1, name: \"test\")";
        let type_info = TypeInfo {
            name: "User".to_string(),
            kind: TypeKind::Struct(vec![]),
            docs: None,
            source_file: None,
            line: None,
            column: None,
            has_default: false,
            ..Default::default()
        };
        let url = Url::parse("file:///test.ron").unwrap();

        let action = create_explicit_root_type_action(&parse(content), content, &type_info, &url);
        assert!(action.is_some());

        if let Some(CodeActionOrCommand::CodeAction(action)) = action {
            assert_eq!(action.title, "Make struct name explicit: User");
            let edit = action.edit.unwrap();
            let changes = edit.changes.unwrap();
            let text_edits = changes.values().next().unwrap();
            assert_eq!(text_edits.len(), 1);
            assert_eq!(text_edits[0].new_text, "User");
            assert_eq!(text_edits[0].range.start.line, 0);
            assert_eq!(text_edits[0].range.start.character, 0);
        }
    }

    #[test]
    fn test_explicit_root_type_next_line() {
        let content = "(\n    id: 1,\n    name: \"test\"\n)";
        let type_info = TypeInfo {
            name: "example::User".to_string(),
            kind: TypeKind::Struct(vec![]),
            docs: None,
            source_file: None,
            line: None,
            column: None,
            has_default: false,
            ..Default::default()
        };
        let url = Url::parse("file:///test.ron").unwrap();

        let action = create_explicit_root_type_action(&parse(content), content, &type_info, &url);
        assert!(action.is_some());

        if let Some(CodeActionOrCommand::CodeAction(action)) = action {
            assert_eq!(action.title, "Make struct name explicit: User");
            let edit = action.edit.unwrap();
            let changes = edit.changes.unwrap();
            let text_edits = changes.values().next().unwrap();
            assert_eq!(text_edits.len(), 1);
            assert_eq!(text_edits[0].new_text, "User");
            assert_eq!(text_edits[0].range.start.line, 0);
            assert_eq!(text_edits[0].range.start.character, 0);
        }
    }

    #[test]
    fn test_explicit_field_type_same_line() {
        let content = "User(author: (id: 1, name: \"test\"))";
        let field = FieldInfo {
            name: "author".to_string(),
            type_name: "Author".to_string(),
            docs: None,
            line: None,
            column: None,
            has_default: false,
            ..Default::default()
        };
        let url = Url::parse("file:///test.ron").unwrap();

        let action = create_explicit_field_type_action(&parse(content), content, &field, None, &url);
        assert!(action.is_some());

        if let Some(CodeActionOrCommand::CodeAction(action)) = action {
            assert_eq!(action.title, "Make field type explicit: author Author");
            let edit = action.edit.unwrap();
            let changes = edit.changes.unwrap();
            let text_edits = changes.values().next().unwrap();
            assert_eq!(text_edits.len(), 1);
            assert_eq!(text_edits[0].new_text, "Author");
            // Should insert before the opening paren after "author: "
            assert_eq!(text_edits[0].range.start.line, 0);
            assert_eq!(text_edits[0].range.start.character, 13); // position of '(' after "author: "
        }
    }

    #[test]
    fn test_explicit_field_type_next_line() {
        let content = r#"Post(
    author: (
        id: 5,
        name: "Charlie",
        email: "charlie@example.com"
    )
)"#;
        let field = FieldInfo {
            name: "author".to_string(),
            type_name: "User".to_string(),
            docs: None,
            line: None,
            column: None,
            has_default: false,
            ..Default::default()
        };
        let url = Url::parse("file:///test.ron").unwrap();

        let action = create_explicit_field_type_action(&parse(content), content, &field, None, &url);
        assert!(action.is_some());

        if let Some(CodeActionOrCommand::CodeAction(action)) = action {
            assert_eq!(action.title, "Make field type explicit: author User");
            let edit = action.edit.unwrap();
            let changes = edit.changes.unwrap();
            let text_edits = changes.values().next().unwrap();
            assert_eq!(text_edits.len(), 1);
            assert_eq!(text_edits[0].new_text, "User");
            // Should insert at the opening paren on line 1
            assert_eq!(text_edits[0].range.start.line, 1);
            assert_eq!(text_edits[0].range.start.character, 12); // position of '(' on second line
        }
    }

    #[test]
    fn test_explicit_field_type_with_option_wrapper() {
        let content = "User(author: (id: 1, name: \"test\"))";
        let field = FieldInfo {
            name: "author".to_string(),
            type_name: "Option<Author>".to_string(),
            docs: None,
            line: None,
            column: None,
            has_default: false,
            ..Default::default()
        };
        let url = Url::parse("file:///test.ron").unwrap();

        let action = create_explicit_field_type_action(&parse(content), content, &field, None, &url);
        assert!(action.is_some());

        if let Some(CodeActionOrCommand::CodeAction(action)) = action {
            assert_eq!(action.title, "Make field type explicit: author Author");
            let edit = action.edit.unwrap();
            let changes = edit.changes.unwrap();
            let text_edits = changes.values().next().unwrap();
            assert_eq!(text_edits[0].new_text, "Author"); // Should strip Option wrapper
        }
    }

    #[test]
    fn test_no_action_for_explicit_type() {
        let content = "User(id: 1, name: \"test\")";
        let type_info = TypeInfo {
            name: "User".to_string(),
            kind: TypeKind::Struct(vec![]),
            docs: None,
            source_file: None,
            line: None,
            column: None,
            has_default: false,
            ..Default::default()
        };
        let url = Url::parse("file:///test.ron").unwrap();

        // Should not offer action since type is already explicit
        let action = create_explicit_root_type_action(&parse(content), content, &type_info, &url);
        assert!(action.is_none());
    }

    #[test]
    fn test_no_action_for_explicit_field_type() {
        let content = "User(author: Author(id: 1, name: \"test\"))";
        let field = FieldInfo {
            name: "author".to_string(),
            type_name: "Author".to_string(),
            docs: None,
            line: None,
            column: None,
            has_default: false,
            ..Default::default()
        };
        let url = Url::parse("file:///test.ron").unwrap();

        // Should not offer action since field type is already explicit
        let action = create_explicit_field_type_action(&parse(content), content, &field, None, &url);
        assert!(action.is_none());
    }

    #[test]
    fn test_real_world_with_comments_and_spacing() {
        let content = r#"Post(
    id: 123,
    title: "Mixed Syntax Example",
    content: "Demonstrating both explicit and unnamed struct syntax",

    // Explicit type name for author
    author: (
        id: 5,
        name: "Charlie",
        email: "charlie@example.com",
        age: 28,
        bio: None,
        is_active: true,
        roles: ["editor"],
    ),

    likes: 50,
    tags: ["example", "syntax"],
    published: true,
    post_type: Short,
)"#;
        let field = FieldInfo {
            name: "author".to_string(),
            type_name: "User".to_string(),
            docs: None,
            line: None,
            column: None,
            has_default: false,
            ..Default::default()
        };
        let url = Url::parse("file:///test.ron").unwrap();

        let action = create_explicit_field_type_action(&parse(content), content, &field, None, &url);
        assert!(action.is_some());

        if let Some(CodeActionOrCommand::CodeAction(action)) = action {
            assert_eq!(action.title, "Make field type explicit: author User");
            let edit = action.edit.unwrap();
            let changes = edit.changes.unwrap();
            let text_edits = changes.values().next().unwrap();
            assert_eq!(text_edits.len(), 1);
            assert_eq!(text_edits[0].new_text, "User");
            // The opening paren is on line 6 (0-indexed), after the comment
            assert_eq!(text_edits[0].range.start.line, 6);
            // The opening paren is at column 12 (after "    author: ")
            assert_eq!(text_edits[0].range.start.character, 12);

            println!(
                "Line: {}, Character: {}",
                text_edits[0].range.start.line, text_edits[0].range.start.character
            );
        }
    }

    #[test]
    fn test_enum_variant_missing_fields() {
        use crate::rust_analyzer::EnumVariant;

        let variant = EnumVariant {
            name: "StructVariant".to_string(),
            fields: vec![
                FieldInfo {
                    name: "field_a".to_string(),
                    type_name: "String".to_string(),
                    docs: None,
                    line: Some(10),
                    column: Some(8),
                    has_default: false,
                    ..Default::default()
                },
                FieldInfo {
                    name: "field_b".to_string(),
                    type_name: "i32".to_string(),
                    docs: None,
                    line: Some(11),
                    column: Some(8),
                    has_default: false,
                    ..Default::default()
                },
            ],
            docs: None,
            line: Some(9),
            column: Some(4),
            ..Default::default()
        };

        let type_info = TypeInfo {
            name: "MyEnum".to_string(),
            kind: TypeKind::Enum(vec![variant]),
            docs: None,
            source_file: None,
            line: Some(8),
            column: Some(0),
            has_default: false,
            ..Default::default()
        };

        let content = "MyEnum::StructVariant(\n    field_a: \"test\"\n)";
        let url = Url::parse("file:///test.ron").unwrap();

        // Create mock analyzer and client for the test
        use crate::rust_analyzer::RustAnalyzer;
        use std::sync::Arc;

        let analyzer = Arc::new(RustAnalyzer::new());

        let actions = generate_code_actions(&parse(content), content, &type_info, &url, analyzer, &[]);

        // Should suggest adding missing field_b
        assert!(!actions.is_empty());

        let titles: Vec<String> = actions
            .iter()
            .filter_map(|a| {
                if let CodeActionOrCommand::CodeAction(action) = a {
                    Some(action.title.clone())
                } else {
                    None
                }
            })
            .collect();

        // Should have action mentioning the variant name
        assert!(titles.iter().any(|t| t.contains("StructVariant")));
        assert!(titles.iter().any(|t| t.contains("field")));
    }

    #[test]
    fn test_detect_current_variant_in_content() {
        let content = "MyEnum::StructVariant(\n    field_a: value\n)";
        let variant = detect_current_variant_in_content(content);
        println!("Detected variant: {:?}", variant);
        assert_eq!(variant, Some("StructVariant".to_string()));
    }

    #[test]
    fn test_detect_current_variant_in_content_no_match() {
        let content = "MyStruct(\n    field_a: value\n)";
        let variant = detect_current_variant_in_content(content);
        println!("Detected variant (should be None): {:?}", variant);
        assert!(variant.is_none());
    }

    #[test]
    fn test_remove_unknown_field_action() {
        let content = "(\n    name: \"a\",\n    bogus: 1,\n)";
        let url = Url::parse("file:///test.ron").unwrap();
        let diagnostic = Diagnostic {
            range: Range::new(Position::new(2, 4), Position::new(2, 9)),
            severity: Some(DiagnosticSeverity::ERROR),
            message: "Unknown field 'bogus'".to_string(),
            code: Some(NumberOrString::String("unknown-field".to_string())),
            ..Default::default()
        };

        let actions =
            generate_remove_field_actions(&parse(content), content, &[diagnostic], &url);
        assert_eq!(actions.len(), 1);
        let CodeActionOrCommand::CodeAction(action) = &actions[0] else {
            panic!("expected code action");
        };
        assert_eq!(action.title, "Remove field 'bogus'");

        let edits = &action.edit.as_ref().unwrap().changes.as_ref().unwrap()[&url];
        assert_eq!(edits.len(), 1);
        // The field is alone on its line: the whole line is removed
        assert_eq!(edits[0].new_text, "");
        assert_eq!(edits[0].range.start, Position::new(2, 0));
        assert_eq!(edits[0].range.end, Position::new(3, 0));
    }

    #[test]
    fn test_remove_field_action_ignores_other_codes() {
        let content = "(name: \"a\")";
        let url = Url::parse("file:///test.ron").unwrap();
        let diagnostic = Diagnostic {
            range: Range::new(Position::new(0, 1), Position::new(0, 5)),
            message: "Type mismatch: expected u32".to_string(),
            code: Some(NumberOrString::String("type-mismatch".to_string())),
            ..Default::default()
        };

        let actions =
            generate_remove_field_actions(&parse(content), content, &[diagnostic], &url);
        assert!(actions.is_empty());
    }
}
