//! Navigation through nested RON type contexts.
use super::tree_sitter_parser::TypeContext;
use super::type_utils::short_name;
use crate::rust_analyzer::{RustAnalyzer, TypeInfo};

/// Navigate through nested type contexts to find the innermost type.
/// The first context corresponds to `start`, which the caller has already
/// resolved; navigation begins from the second context.
pub fn navigate_type_contexts(
    analyzer: &RustAnalyzer,
    start: Option<TypeInfo>,
    contexts: &[TypeContext],
) -> Option<TypeInfo> {
    let mut current_type_info = start;

    for context in contexts.iter().skip(1) {
        let info = match current_type_info {
            Some(ref info) => info.clone(),
            None => break,
        };

        // Try to find the context type as a field's type.
        // Use exact match on the last component of the type name to avoid
        // substring matches (e.g., don't match "Post" with "PostType").
        if let Some(fields) = info.fields() {
            let context_name = &context.type_name;
            if let Some(field) = fields.iter().find(|f| {
                let field_type_last = short_name(&f.type_name);
                // Remove generic parameters for comparison
                let field_type_base = field_type_last.split('<').next().unwrap_or(field_type_last);
                field_type_base == context_name
            }) {
                current_type_info = analyzer.get_type_info(&field.type_name).cloned();
                continue;
            }
        }

        // Try as direct type lookup
        let direct_lookup = analyzer.get_type_info(&context.type_name).cloned();
        if direct_lookup.is_some() {
            current_type_info = direct_lookup;
        } else {
            // The context name might be a variant name
            let mut found_via_variant = false;

            // Check if current type is an enum with this variant
            if let Some(variant) = info.find_variant(&context.type_name)
                && variant.fields.len() == 1
            {
                // For tuple variants with one field, navigate to that field's type
                let field_type = &variant.fields[0].type_name;
                current_type_info = analyzer.get_type_info(field_type).cloned();
                found_via_variant = true;
            }

            // If not found as a variant of the current type, try to find it in field types
            if !found_via_variant && let Some(fields) = info.fields() {
                for field in fields {
                    if let Some(field_type_info) = analyzer.get_type_info(&field.type_name).cloned()
                        && field_type_info.find_variant(&context.type_name).is_some()
                    {
                        current_type_info = Some(field_type_info);
                        found_via_variant = true;
                        break;
                    }
                }
            }

            if !found_via_variant {
                current_type_info = None;
            }
        }
    }

    current_type_info
}
