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
pub fn strip_outer_generic(type_name: &str) -> String {
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

/// Check if a type is a user-defined struct or enum — i.e. something the
/// analyzer should resolve and validate against. This is exactly the types that
/// are neither a primitive ([`is_primitive_type`]) nor a standard-library
/// generic wrapper ([`is_std_generic_type`]). Both of those normalize
/// whitespace internally, so `type_name` may be passed raw or pre-normalized.
pub fn is_custom_type(type_name: &str) -> bool {
    !is_primitive_type(type_name) && !is_std_generic_type(type_name)
}

/// Largest absolute edit distance we ever treat as a plausible typo. Real
/// misspellings are almost always one or two edits away; beyond that the
/// "match" is a different word, so suggesting it is just noise.
const MAX_EDIT_DISTANCE: usize = 2;

/// Optimal String Alignment distance between two strings, compared
/// case-insensitively. This is Levenshtein extended so that an adjacent
/// transposition (a very common typo, e.g. `prot` for `port`) costs one edit
/// instead of two. Used to power "did you mean?" suggestions.
fn edit_distance(a: &str, b: &str) -> usize {
    let a: String = a.chars().flat_map(char::to_lowercase).collect();
    let b: String = b.chars().flat_map(char::to_lowercase).collect();
    strsim::osa_distance(&a, &b)
}

/// Find the candidate most similar to `target` for a "did you mean?" hint.
///
/// Returns the closest candidate whose edit distance is small enough to be a
/// plausible typo. To keep suggestions from becoming noise, the match must be
/// genuinely close: at most [`MAX_EDIT_DISTANCE`] edits, no more than a third
/// of the longer name's length, and strictly less than `target`'s length so an
/// entirely different word is never suggested. The bounds mean very short names
/// (≤2 chars) only match exactly. Returns `None` when nothing is close enough
/// or `candidates` is empty; ties break to the first occurrence.
pub fn closest_name<'a>(
    target: &str,
    candidates: impl IntoIterator<Item = &'a str>,
) -> Option<&'a str> {
    let target_len = target.chars().count();
    let mut best: Option<(&str, usize)> = None;

    for candidate in candidates {
        let dist = edit_distance(target, candidate);
        let longer = target_len.max(candidate.chars().count());
        let threshold = (longer / 3).min(MAX_EDIT_DISTANCE);

        if dist <= threshold
            && dist < target_len
            && best.is_none_or(|(_, best_dist)| dist < best_dist)
        {
            best = Some((candidate, dist));
        }
    }

    best.map(|(name, _)| name)
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
    fn test_strip_outer_generic() {
        assert_eq!(strip_outer_generic("Option<Post>"), "Post");
        assert_eq!(strip_outer_generic("Vec < Post >"), "Post");
        assert_eq!(strip_outer_generic("Post"), "Post");
        // Only the outermost layer is stripped.
        assert_eq!(strip_outer_generic("Option<Vec<Post>>"), "Vec<Post>");
    }

    #[test]
    fn test_edit_distance() {
        assert_eq!(edit_distance("name", "name"), 0);
        assert_eq!(edit_distance("nam", "name"), 1);
        assert_eq!(edit_distance("Name", "name"), 0); // case-insensitive
        assert_eq!(edit_distance("", "abc"), 3);
        assert_eq!(edit_distance("kitten", "sitting"), 3);
    }

    #[test]
    fn test_closest_name_suggests_typo() {
        let fields = ["ephemeral", "database", "timeout"];
        assert_eq!(closest_name("ephemerl", fields), Some("ephemeral"));
        assert_eq!(closest_name("databse", fields), Some("database"));
        // Case-only difference is still a match.
        assert_eq!(closest_name("Timeout", fields), Some("timeout"));
    }

    #[test]
    fn test_closest_name_rejects_unrelated() {
        let fields = ["ephemeral", "database", "timeout"];
        // Nothing close enough to be a plausible typo.
        assert_eq!(closest_name("hostname", fields), None);
        // A completely different short word should not match a short candidate.
        assert_eq!(closest_name("id", ["ip"]), None);
        // No candidates.
        assert_eq!(closest_name("anything", std::iter::empty()), None);
        // Three edits on a long name: within the old length/3 bound but past
        // the absolute cap, so it is no longer treated as a plausible typo.
        assert_eq!(closest_name("abcdefXYZj", ["abcdefghij"]), None);
    }

    #[test]
    fn test_closest_name_picks_nearest() {
        // "prot" is one edit from "port" but two from "protocol"; pick the nearest.
        assert_eq!(closest_name("prot", ["port", "protocol"]), Some("port"));
    }
}
