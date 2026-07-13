//! Shared helpers for working with Rust type names extracted by the analyzer.
use crate::rust_analyzer::FieldInfo;

/// Get the last path segment of a fully-qualified type name.
/// For example: `crate::models::User` -> `User`
pub fn short_name(path: &str) -> &str {
    path.rsplit("::").next().unwrap_or(path)
}

/// Extract the inner type from a generic type when it starts with the given
/// wrapper prefix. For example: `extract_inner_type("Vec<User>", "Vec<")` -> `Some("User")`
pub fn extract_inner_type<'a>(type_str: &'a str, wrapper: &str) -> Option<&'a str> {
    if type_str.starts_with(wrapper) && type_str.ends_with('>') {
        Some(&type_str[wrapper.len()..type_str.len() - 1])
    } else {
        None
    }
}

/// Get the content of the outermost generic (e.g., `Option<Vec<T>>` -> `Vec<T>`),
/// or the type itself if it isn't generic. Whitespace is removed.
pub fn innermost_generic(type_name: &str) -> String {
    let clean = type_name.replace(' ', "");
    match (clean.find('<'), clean.rfind('>')) {
        (Some(start), Some(end)) if start < end => clean[start + 1..end].to_string(),
        _ => clean,
    }
}

/// Check if a type is a primitive type (not a custom enum/struct)
pub fn is_primitive_type(type_name: &str) -> bool {
    let clean = type_name.replace(" ", "");

    let primitives = [
        "bool", "i8", "i16", "i32", "i64", "i128", "isize", "u8", "u16", "u32", "u64", "u128",
        "usize", "f32", "f64", "char", "String", "&str", "str",
    ];

    primitives.contains(&clean.as_str())
}

/// Check if a type is a standard library generic type (Option, Vec, HashMap, etc.)
pub fn is_std_generic_type(type_name: &str) -> bool {
    let clean = type_name.replace(" ", "");

    clean.starts_with("Option<")
        || clean.starts_with("Vec<")
        || clean.contains("HashMap<")
        || clean.contains("BTreeMap<")
        || clean.contains("HashSet<")
        || clean.contains("BTreeSet<")
        || clean.starts_with("Result<")
        || clean.starts_with("Box<")
        || clean.starts_with("Rc<")
        || clean.starts_with("Arc<")
}

/// Fields that must be present in the RON: not `Option<T>`, no default.
/// Takes `(serialized_name, field)` pairs (see `TypeInfo::effective_fields` /
/// `EnumVariant::effective_fields`); a field counts as present under either
/// its serialized or Rust name.
pub fn missing_required_fields(
    fields: &[(String, FieldInfo)],
    is_present: impl Fn(&str) -> bool,
) -> Vec<(String, FieldInfo)> {
    fields
        .iter()
        .filter(|(name, f)| !is_present(name) && !is_present(&f.name) && !f.is_optional())
        .cloned()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_short_name() {
        assert_eq!(short_name("crate::models::User"), "User");
        assert_eq!(short_name("User"), "User");
    }

    #[test]
    fn test_extract_inner_type() {
        assert_eq!(extract_inner_type("Vec<User>", "Vec<"), Some("User"));
        assert_eq!(
            extract_inner_type("Option<Vec<i32>>", "Option<"),
            Some("Vec<i32>")
        );
        assert_eq!(extract_inner_type("Vec<User>", "Option<"), None);
        assert_eq!(extract_inner_type("User", "Vec<"), None);
    }

    #[test]
    fn test_innermost_generic() {
        assert_eq!(innermost_generic("Option<Post>"), "Post");
        assert_eq!(innermost_generic("Vec < Post >"), "Post");
        assert_eq!(innermost_generic("Post"), "Post");
    }
}
