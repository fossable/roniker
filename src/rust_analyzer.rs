use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Component, Path, PathBuf};
use syn::{Attribute, Fields, Item, ItemEnum, ItemStruct, ItemType, Type};
use tokio::sync::RwLock;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FieldInfo {
    pub name: String,
    pub type_name: String,
    pub docs: Option<String>,
    pub line: Option<usize>,
    pub column: Option<usize>,
    pub has_default: bool,
}

impl FieldInfo {
    pub fn is_optional(&self) -> bool {
        self.type_name.starts_with("Option") || self.has_default
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnumVariant {
    pub name: String,
    pub fields: Vec<FieldInfo>,
    pub docs: Option<String>,
    pub line: Option<usize>,
    pub column: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TypeKind {
    Struct(Vec<FieldInfo>),
    Enum(Vec<EnumVariant>),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TypeInfo {
    pub name: String,
    pub kind: TypeKind,
    pub docs: Option<String>,
    pub source_file: Option<PathBuf>,
    pub line: Option<usize>,
    pub column: Option<usize>,
    pub has_default: bool,
}

impl TypeInfo {
    pub fn fields(&self) -> Option<&Vec<FieldInfo>> {
        match &self.kind {
            TypeKind::Struct(fields) => Some(fields),
            TypeKind::Enum(_) => None,
        }
    }

    pub fn find_field(&self, field_name: &str) -> Option<&FieldInfo> {
        match &self.kind {
            TypeKind::Struct(fields) => fields.iter().find(|f| f.name == field_name),
            TypeKind::Enum(variants) => {
                // Search through all variants' fields
                variants
                    .iter()
                    .flat_map(|v| &v.fields)
                    .find(|f| f.name == field_name)
            }
        }
    }

    pub fn find_variant(&self, variant_name: &str) -> Option<&EnumVariant> {
        match &self.kind {
            TypeKind::Enum(variants) => variants.iter().find(|v| v.name == variant_name),
            TypeKind::Struct(_) => None,
        }
    }
}

pub struct RustAnalyzer {
    root_type: RwLock<Option<String>>,
    type_cache: RwLock<HashMap<String, TypeInfo>>,
    type_aliases: RwLock<HashMap<String, String>>,
}

impl serde::Serialize for RustAnalyzer {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;

        let root_type = self
            .root_type
            .try_read()
            .map_err(|_| serde::ser::Error::custom("failed to acquire root_type lock"))?;
        let type_cache = self
            .type_cache
            .try_read()
            .map_err(|_| serde::ser::Error::custom("failed to acquire type_cache lock"))?;
        let type_aliases = self
            .type_aliases
            .try_read()
            .map_err(|_| serde::ser::Error::custom("failed to acquire type_aliases lock"))?;

        let mut state = serializer.serialize_struct("RustAnalyzer", 3)?;
        state.serialize_field("root_type", &*root_type)?;
        state.serialize_field("type_cache", &*type_cache)?;
        state.serialize_field("type_aliases", &*type_aliases)?;
        state.end()
    }
}

impl<'de> serde::Deserialize<'de> for RustAnalyzer {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        use serde::de::{self, MapAccess, Visitor};
        use std::fmt;

        #[derive(serde::Deserialize)]
        #[serde(field_identifier, rename_all = "snake_case")]
        enum Field {
            RootType,
            TypeCache,
            TypeAliases,
        }

        struct RustAnalyzerVisitor;

        impl<'de> Visitor<'de> for RustAnalyzerVisitor {
            type Value = RustAnalyzer;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("struct RustAnalyzer")
            }

            fn visit_map<V>(self, mut map: V) -> Result<RustAnalyzer, V::Error>
            where
                V: MapAccess<'de>,
            {
                let mut root_type = None;
                let mut type_cache = None;
                let mut type_aliases = None;

                while let Some(key) = map.next_key()? {
                    match key {
                        Field::RootType => {
                            if root_type.is_some() {
                                return Err(de::Error::duplicate_field("root_type"));
                            }
                            root_type = Some(map.next_value()?);
                        }
                        Field::TypeCache => {
                            if type_cache.is_some() {
                                return Err(de::Error::duplicate_field("type_cache"));
                            }
                            type_cache = Some(map.next_value()?);
                        }
                        Field::TypeAliases => {
                            if type_aliases.is_some() {
                                return Err(de::Error::duplicate_field("type_aliases"));
                            }
                            type_aliases = Some(map.next_value()?);
                        }
                    }
                }

                let root_type =
                    root_type.ok_or_else(|| de::Error::missing_field("root_type"))?;
                let type_cache =
                    type_cache.ok_or_else(|| de::Error::missing_field("type_cache"))?;
                let type_aliases =
                    type_aliases.ok_or_else(|| de::Error::missing_field("type_aliases"))?;

                Ok(RustAnalyzer {
                    root_type: RwLock::new(root_type),
                    type_cache: RwLock::new(type_cache),
                    type_aliases: RwLock::new(type_aliases),
                })
            }
        }

        const FIELDS: &[&str] = &["root_type", "type_cache", "type_aliases"];
        deserializer.deserialize_struct("RustAnalyzer", FIELDS, RustAnalyzerVisitor)
    }
}

impl RustAnalyzer {
    pub fn new() -> Self {
        Self {
            root_type: RwLock::new(None),
            type_cache: RwLock::new(HashMap::new()),
            type_aliases: RwLock::new(HashMap::new()),
        }
    }

    /// Set the root type path for this analyzer (e.g., "crate::models::Config")
    pub async fn set_root_type(&self, type_path: &str) {
        *self.root_type.write().await = Some(type_path.to_string());
    }

    /// Get the root type path
    pub async fn get_root_type(&self) -> Option<String> {
        self.root_type.read().await.clone()
    }

    /// Get the TypeInfo for the root type
    pub async fn get_root_type_info(&self) -> Option<TypeInfo> {
        let root_type = self.get_root_type().await?;
        self.get_type_info(&root_type).await
    }

    /// Add a Rust source file to the analyzer.
    ///
    /// Parses the file and extracts all type definitions (structs, enums, type aliases).
    /// The module path is inferred from the file path (e.g., `src/models/user.rs` -> `crate::models::user`).
    ///
    /// # Arguments
    /// * `file_path` - Path to the Rust source file
    ///
    /// # Returns
    /// * `Ok(usize)` - Number of types extracted from the file
    /// * `Err` - If the file cannot be read or parsed
    pub async fn add_file(&self, file_path: &Path) -> Result<usize> {
        let content = fs::read_to_string(file_path)
            .with_context(|| format!("Failed to read file: {}", file_path.display()))?;
        self.add_source(file_path, &content).await
    }

    /// Add Rust source code to the analyzer with an associated file path.
    ///
    /// Parses the source and extracts all type definitions.
    /// The module path is inferred from the file path.
    ///
    /// # Arguments
    /// * `file_path` - Path to associate with this source (used for module path inference)
    /// * `source` - Rust source code to parse
    ///
    /// # Returns
    /// * `Ok(usize)` - Number of types extracted from the source
    /// * `Err` - If the source cannot be parsed
    pub async fn add_source(&self, file_path: &Path, source: &str) -> Result<usize> {
        let syntax_tree = syn::parse_file(source)
            .with_context(|| format!("Failed to parse Rust source from: {}", file_path.display()))?;

        let mut type_cache = self.type_cache.write().await;
        let mut type_aliases = self.type_aliases.write().await;

        let initial_count = type_cache.len();
        self.extract_types_from_file(&syntax_tree, file_path, &mut type_cache, &mut type_aliases);
        let types_added = type_cache.len() - initial_count;

        Ok(types_added)
    }

    /// Add Rust source code with a custom module prefix.
    ///
    /// Unlike `add_source`, this allows you to specify the module path directly
    /// instead of inferring it from a file path.
    ///
    /// # Arguments
    /// * `module_prefix` - The module path prefix (e.g., "crate::models")
    /// * `source` - Rust source code to parse
    ///
    /// # Returns
    /// * `Ok(usize)` - Number of types extracted from the source
    /// * `Err` - If the source cannot be parsed
    pub async fn add_source_with_prefix(
        &self,
        module_prefix: &str,
        source: &str,
    ) -> Result<usize> {
        let syntax_tree = syn::parse_file(source)
            .context("Failed to parse Rust source")?;

        let mut type_cache = self.type_cache.write().await;
        let mut type_aliases = self.type_aliases.write().await;

        let initial_count = type_cache.len();
        self.extract_types_from_syntax_tree(
            &syntax_tree,
            module_prefix,
            None,
            &mut type_cache,
            &mut type_aliases,
        );
        let types_added = type_cache.len() - initial_count;

        Ok(types_added)
    }

    /// Register a type directly with the analyzer.
    ///
    /// This is useful when you have pre-constructed TypeInfo objects.
    pub async fn add_type(&self, type_info: TypeInfo) {
        let mut cache = self.type_cache.write().await;
        cache.insert(type_info.name.clone(), type_info);
    }

    /// Register a type alias.
    ///
    /// # Arguments
    /// * `alias` - The alias name (e.g., "crate::MyAlias")
    /// * `target` - The target type (e.g., "crate::SomeType")
    pub async fn add_type_alias(&self, alias: &str, target: &str) {
        let mut aliases = self.type_aliases.write().await;
        aliases.insert(alias.to_string(), target.to_string());
    }

    /// Remove a type from the analyzer.
    ///
    /// # Returns
    /// The removed TypeInfo if it existed
    pub async fn remove_type(&self, type_path: &str) -> Option<TypeInfo> {
        let mut cache = self.type_cache.write().await;
        cache.remove(type_path)
    }

    /// Clear all types and aliases from the analyzer.
    pub async fn clear(&self) {
        let mut type_cache = self.type_cache.write().await;
        let mut type_aliases = self.type_aliases.write().await;
        type_cache.clear();
        type_aliases.clear();
    }

    fn extract_types_from_file(
        &self,
        syntax_tree: &syn::File,
        file_path: &Path,
        type_cache: &mut HashMap<String, TypeInfo>,
        type_aliases: &mut HashMap<String, String>,
    ) {
        // Extract module path from file path
        let module_prefix = self.file_path_to_module_path(file_path);
        self.extract_types_from_syntax_tree(
            syntax_tree,
            &module_prefix,
            Some(file_path),
            type_cache,
            type_aliases,
        );
    }

    fn extract_types_from_syntax_tree(
        &self,
        syntax_tree: &syn::File,
        module_prefix: &str,
        file_path: Option<&Path>,
        type_cache: &mut HashMap<String, TypeInfo>,
        type_aliases: &mut HashMap<String, String>,
    ) {
        for item in &syntax_tree.items {
            if let Item::Struct(struct_item) = item {
                if let Some(type_info) =
                    self.extract_struct_info(struct_item, module_prefix, file_path)
                {
                    type_cache.insert(type_info.name.clone(), type_info);
                }
            } else if let Item::Enum(enum_item) = item {
                if let Some(type_info) =
                    self.extract_enum_info(enum_item, module_prefix, file_path)
                {
                    type_cache.insert(type_info.name.clone(), type_info);
                }
            } else if let Item::Type(type_item) = item {
                self.extract_type_alias(type_item, module_prefix, type_aliases);
            } else if let Item::Mod(mod_item) = item {
                // Handle inline modules
                if let Some((_, items)) = &mod_item.content {
                    let mod_name = mod_item.ident.to_string();
                    let nested_prefix = if module_prefix.is_empty() {
                        mod_name
                    } else {
                        format!("{}::{}", module_prefix, mod_name)
                    };

                    for item in items {
                        if let Item::Struct(struct_item) = item {
                            if let Some(type_info) =
                                self.extract_struct_info(struct_item, &nested_prefix, file_path)
                            {
                                type_cache.insert(type_info.name.clone(), type_info);
                            }
                        } else if let Item::Enum(enum_item) = item {
                            if let Some(type_info) =
                                self.extract_enum_info(enum_item, &nested_prefix, file_path)
                            {
                                type_cache.insert(type_info.name.clone(), type_info);
                            }
                        } else if let Item::Type(type_item) = item {
                            self.extract_type_alias(type_item, &nested_prefix, type_aliases);
                        }
                    }
                }
            }
        }
    }

    /// Convert a file path to a module path
    ///
    /// Example: `"src/models/user.rs"` -> `"crate::models::user"`
    fn file_path_to_module_path(&self, file_path: &Path) -> String {
        let components: Vec<_> = file_path
            .components()
            .filter_map(|c| match c {
                Component::Normal(os) => os.to_str(),
                _ => None,
            })
            .collect();

        if let Some(src_index) = components.iter().position(|c| *c == "src") {
            let relative = &components[src_index + 1..];
            if relative.is_empty() {
                return "crate".to_string();
            }

            let mut parts = relative.to_vec();
            if let Some(last) = parts.last_mut() {
                if last.ends_with(".rs") {
                    let stem = last.trim_end_matches(".rs");
                    *last = stem;
                }
                if *last == "lib" || *last == "main" {
                    parts.pop();
                    if parts.is_empty() {
                        return "crate".to_string();
                    }
                } else if *last == "mod" {
                    parts.pop();
                }
            }

            if parts.is_empty() {
                "crate".to_string()
            } else {
                format!("crate::{}", parts.join("::"))
            }
        } else {
            String::new()
        }
    }

    fn extract_struct_info(
        &self,
        struct_item: &ItemStruct,
        module_prefix: &str,
        file_path: Option<&Path>,
    ) -> Option<TypeInfo> {
        let struct_name = struct_item.ident.to_string();
        let full_path = if module_prefix.is_empty() {
            struct_name.clone()
        } else {
            format!("{}::{}", module_prefix, struct_name)
        };

        let docs = extract_docs(&struct_item.attrs);

        let fields = match &struct_item.fields {
            Fields::Named(fields) => fields
                .named
                .iter()
                .map(|field| {
                    let field_name = field.ident.as_ref().unwrap().to_string();
                    let type_name = type_to_string(&field.ty);
                    let field_docs = extract_docs(&field.attrs);
                    let (line, column) = field
                        .ident
                        .as_ref()
                        .map(|i| {
                            let start = i.span().start();
                            (start.line, start.column)
                        })
                        .unzip();

                    let field_attributes = serde_attributes::extract_serde_field_attributes(field);
                    let has_default = field_attributes.has_default;

                    FieldInfo {
                        name: field_name,
                        type_name,
                        docs: field_docs,
                        line,
                        column,
                        has_default,
                    }
                })
                .collect(),
            Fields::Unnamed(fields) => fields
                .unnamed
                .iter()
                .enumerate()
                .map(|(i, field)| {
                    let type_name = type_to_string(&field.ty);

                    let field_attributes = serde_attributes::extract_serde_field_attributes(field);
                    let has_default = field_attributes.has_default;

                    FieldInfo {
                        name: i.to_string(),
                        type_name,
                        docs: None,
                        line: None,
                        column: None,
                        has_default,
                    }
                })
                .collect(),
            Fields::Unit => vec![],
        };

        let start = struct_item.ident.span().start();
        let line = Some(start.line);
        let column = Some(start.column);
        let has_default = has_default_derive(&struct_item.attrs);

        Some(TypeInfo {
            name: full_path,
            kind: TypeKind::Struct(fields),
            docs,
            source_file: file_path.map(|p| p.to_path_buf()),
            line,
            column,
            has_default,
        })
    }

    fn extract_enum_info(
        &self,
        enum_item: &ItemEnum,
        module_prefix: &str,
        file_path: Option<&Path>,
    ) -> Option<TypeInfo> {
        let enum_name = enum_item.ident.to_string();
        let full_path = if module_prefix.is_empty() {
            enum_name.clone()
        } else {
            format!("{}::{}", module_prefix, enum_name)
        };

        let docs = extract_docs(&enum_item.attrs);

        let variants = enum_item
            .variants
            .iter()
            .map(|variant| {
                let variant_name = variant.ident.to_string();
                let variant_docs = extract_docs(&variant.attrs);
                let variant_start = variant.ident.span().start();
                let variant_line = Some(variant_start.line);
                let variant_column = Some(variant_start.column);

                let fields = match &variant.fields {
                    Fields::Named(fields) => fields
                        .named
                        .iter()
                        .map(|field| {
                            let field_name = field.ident.as_ref().unwrap().to_string();
                            let type_name = type_to_string(&field.ty);
                            let field_docs = extract_docs(&field.attrs);
                            let (line, column) = field
                                .ident
                                .as_ref()
                                .map(|i| {
                                    let start = i.span().start();
                                    (start.line, start.column)
                                })
                                .unzip();

                            let field_attributes =
                                serde_attributes::extract_serde_field_attributes(field);
                            let has_default = field_attributes.has_default;

                            FieldInfo {
                                name: field_name,
                                type_name,
                                docs: field_docs,
                                line,
                                column,
                                has_default,
                            }
                        })
                        .collect(),
                    Fields::Unnamed(fields) => fields
                        .unnamed
                        .iter()
                        .enumerate()
                        .map(|(i, field)| {
                            let type_name = type_to_string(&field.ty);

                            let field_attributes =
                                serde_attributes::extract_serde_field_attributes(field);
                            let has_default = field_attributes.has_default;

                            FieldInfo {
                                name: i.to_string(),
                                type_name,
                                docs: None,
                                line: None,
                                column: None,
                                has_default,
                            }
                        })
                        .collect(),
                    Fields::Unit => vec![],
                };

                EnumVariant {
                    name: variant_name,
                    fields,
                    docs: variant_docs,
                    line: variant_line,
                    column: variant_column,
                }
            })
            .collect();

        let start = enum_item.ident.span().start();
        let line = Some(start.line);
        let column = Some(start.column);
        let has_default = has_default_derive(&enum_item.attrs);

        Some(TypeInfo {
            name: full_path,
            kind: TypeKind::Enum(variants),
            docs,
            source_file: file_path.map(|p| p.to_path_buf()),
            line,
            column,
            has_default,
        })
    }

    fn extract_type_alias(
        &self,
        type_item: &ItemType,
        module_prefix: &str,
        type_aliases: &mut HashMap<String, String>,
    ) {
        let alias_name = type_item.ident.to_string();
        let full_alias_path = if module_prefix.is_empty() {
            alias_name.clone()
        } else {
            format!("{}::{}", module_prefix, alias_name)
        };

        let target_type = type_to_string(&type_item.ty);

        type_aliases.insert(full_alias_path, target_type);
    }

    pub async fn get_type_info(&self, type_path: &str) -> Option<TypeInfo> {
        // Resolve type aliases first
        let resolved_type = {
            let aliases = self.type_aliases.read().await;
            aliases
                .get(type_path)
                .cloned()
                .unwrap_or_else(|| type_path.to_string())
        };

        // Check cache with exact match
        let cache = self.type_cache.read().await;
        if let Some(info) = cache.get(&resolved_type) {
            return Some(info.clone());
        }
        // Also try the original type path
        if let Some(info) = cache.get(type_path) {
            return Some(info.clone());
        }

        // If not found by exact match, try finding by simple name
        // e.g., "PostType" should match "crate::models::PostType"
        for (key, value) in cache.iter() {
            if key.ends_with(&format!("::{}", type_path))
                || key.ends_with(&format!("::{}", resolved_type))
                || key == type_path
                || key == &resolved_type
            {
                return Some(value.clone());
            }
        }

        None
    }

    /// Get all types registered with the analyzer
    pub async fn get_all_types(&self) -> Vec<TypeInfo> {
        let cache = self.type_cache.read().await;
        cache.values().cloned().collect()
    }

    /// Get the number of types registered with the analyzer
    pub async fn type_count(&self) -> usize {
        let cache = self.type_cache.read().await;
        cache.len()
    }

    /// Check if a type exists in the analyzer
    pub async fn has_type(&self, type_path: &str) -> bool {
        self.get_type_info(type_path).await.is_some()
    }

    #[cfg(test)]
    pub async fn insert_type_for_test(&self, type_info: TypeInfo) {
        self.add_type(type_info).await;
    }
}

impl Default for RustAnalyzer {
    fn default() -> Self {
        Self::new()
    }
}

fn extract_docs(attrs: &[Attribute]) -> Option<String> {
    let docs: Vec<String> = attrs
        .iter()
        .filter_map(|attr| {
            if attr.path().is_ident("doc") {
                attr.meta.require_name_value().ok().and_then(|nv| {
                    if let syn::Expr::Lit(lit) = &nv.value {
                        if let syn::Lit::Str(s) = &lit.lit {
                            return Some(s.value().trim().to_string());
                        }
                    }
                    None
                })
            } else {
                None
            }
        })
        .collect();

    if docs.is_empty() {
        None
    } else {
        Some(docs.join("\n"))
    }
}

fn has_default_derive(attrs: &[Attribute]) -> bool {
    attrs.iter().any(|attr| {
        if attr.path().is_ident("derive") {
            // Parse the derive attribute to check for Default
            if let Ok(meta_list) = attr.meta.require_list() {
                let tokens_str = meta_list.tokens.to_string();
                // Check if "Default" appears in the derive list
                tokens_str.split(',').any(|s| s.trim() == "Default")
            } else {
                false
            }
        } else {
            false
        }
    })
}

fn type_to_string(ty: &Type) -> String {
    match ty {
        Type::Path(type_path) => quote::quote!(#type_path).to_string(),
        Type::Reference(type_ref) => {
            let inner = type_to_string(&type_ref.elem);
            if type_ref.mutability.is_some() {
                format!("&mut {}", inner)
            } else {
                format!("&{}", inner)
            }
        }
        _ => quote::quote!(#ty).to_string(),
    }
}

mod serde_attributes {
    use syn::Field;

    pub struct SerdeFieldAttributes {
        pub has_default: bool,
    }

    pub fn extract_serde_field_attributes(field: &Field) -> SerdeFieldAttributes {
        let mut field_attrs = SerdeFieldAttributes { has_default: false };

        let attrs = field
            .attrs
            .iter()
            .filter(|attr| attr.path().is_ident("serde"));

        for attr in attrs {
            let _ = attr.parse_nested_meta(|meta| {
                // #[serde(default)]
                if meta.path.is_ident("default") {
                    field_attrs.has_default = true;
                }
                Ok(())
            });
        }

        field_attrs
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[tokio::test]
    async fn test_rust_analyzer_serialization_roundtrip() {
        let analyzer = RustAnalyzer::new();
        analyzer.set_root_type("crate::Test").await;
        analyzer
            .add_type(TypeInfo {
                name: "Test".to_string(),
                kind: TypeKind::Struct(vec![]),
                docs: None,
                source_file: None,
                line: None,
                column: None,
                has_default: false,
            })
            .await;

        let json = serde_json::to_string(&analyzer).unwrap();
        let deserialized: RustAnalyzer = serde_json::from_str(&json).unwrap();

        assert!(deserialized.get_type_info("Test").await.is_some());
        assert_eq!(
            deserialized.get_root_type().await,
            Some("crate::Test".to_string())
        );
    }

    #[tokio::test]
    async fn test_add_source_with_prefix() {
        let analyzer = RustAnalyzer::new();

        let source = r#"
            /// A user in the system
            pub struct User {
                /// The user's unique ID
                pub id: u64,
                pub name: String,
            }

            pub enum Status {
                Active,
                Inactive,
            }
        "#;

        let count = analyzer
            .add_source_with_prefix("crate::models", source)
            .await
            .unwrap();

        assert_eq!(count, 2, "Should extract 2 types (User and Status)");

        let user = analyzer.get_type_info("crate::models::User").await;
        assert!(user.is_some(), "User type should exist");

        let user = user.unwrap();
        assert_eq!(user.docs, Some("A user in the system".to_string()));
        assert!(matches!(user.kind, TypeKind::Struct(_)));

        if let TypeKind::Struct(fields) = &user.kind {
            assert_eq!(fields.len(), 2);
            assert_eq!(fields[0].name, "id");
            assert_eq!(fields[0].docs, Some("The user's unique ID".to_string()));
        }

        let status = analyzer.get_type_info("crate::models::Status").await;
        assert!(status.is_some(), "Status type should exist");
    }

    #[tokio::test]
    async fn test_add_source_with_file_path() {
        let analyzer = RustAnalyzer::new();

        let source = r#"
            pub struct Config {
                pub debug: bool,
            }
        "#;

        let file_path = PathBuf::from("src/settings/config.rs");
        let count = analyzer.add_source(&file_path, source).await.unwrap();

        assert_eq!(count, 1);

        // Should be accessible via the inferred module path
        let config = analyzer
            .get_type_info("crate::settings::config::Config")
            .await;
        assert!(config.is_some(), "Config should be at inferred path");
    }

    #[tokio::test]
    async fn test_add_type_directly() {
        let analyzer = RustAnalyzer::new();

        let type_info = TypeInfo {
            name: "crate::MyType".to_string(),
            kind: TypeKind::Struct(vec![FieldInfo {
                name: "value".to_string(),
                type_name: "i32".to_string(),
                docs: Some("The value".to_string()),
                line: None,
                column: None,
                has_default: false,
            }]),
            docs: Some("My custom type".to_string()),
            source_file: None,
            line: None,
            column: None,
            has_default: false,
        };

        analyzer.add_type(type_info).await;

        assert!(analyzer.has_type("crate::MyType").await);
        assert_eq!(analyzer.type_count().await, 1);
    }

    #[tokio::test]
    async fn test_remove_type() {
        let analyzer = RustAnalyzer::new();

        analyzer
            .add_type(TypeInfo {
                name: "crate::ToRemove".to_string(),
                kind: TypeKind::Struct(vec![]),
                docs: None,
                source_file: None,
                line: None,
                column: None,
                has_default: false,
            })
            .await;

        assert!(analyzer.has_type("crate::ToRemove").await);

        let removed = analyzer.remove_type("crate::ToRemove").await;
        assert!(removed.is_some());
        assert!(!analyzer.has_type("crate::ToRemove").await);
    }

    #[tokio::test]
    async fn test_clear() {
        let analyzer = RustAnalyzer::new();

        analyzer
            .add_source_with_prefix("crate", "pub struct A {} pub struct B {}")
            .await
            .unwrap();

        assert_eq!(analyzer.type_count().await, 2);

        analyzer.clear().await;

        assert_eq!(analyzer.type_count().await, 0);
    }

    #[tokio::test]
    async fn test_type_alias() {
        let analyzer = RustAnalyzer::new();

        analyzer
            .add_type(TypeInfo {
                name: "crate::RealType".to_string(),
                kind: TypeKind::Struct(vec![]),
                docs: Some("The real type".to_string()),
                source_file: None,
                line: None,
                column: None,
                has_default: false,
            })
            .await;

        analyzer
            .add_type_alias("crate::AliasType", "crate::RealType")
            .await;

        // Looking up the alias should return the real type
        let via_alias = analyzer.get_type_info("crate::AliasType").await;
        assert!(via_alias.is_some());
        assert_eq!(via_alias.unwrap().name, "crate::RealType");
    }

    #[tokio::test]
    async fn test_inline_module() {
        let analyzer = RustAnalyzer::new();

        let source = r#"
            pub mod inner {
                pub struct InnerType {
                    pub x: i32,
                }
            }
        "#;

        analyzer
            .add_source_with_prefix("crate", source)
            .await
            .unwrap();

        let inner_type = analyzer.get_type_info("crate::inner::InnerType").await;
        assert!(inner_type.is_some(), "InnerType should be found");
    }

    #[tokio::test]
    async fn test_simple_name_lookup() {
        let analyzer = RustAnalyzer::new();

        analyzer
            .add_type(TypeInfo {
                name: "crate::deeply::nested::MyStruct".to_string(),
                kind: TypeKind::Struct(vec![]),
                docs: None,
                source_file: None,
                line: None,
                column: None,
                has_default: false,
            })
            .await;

        // Should be able to find by simple name
        let found = analyzer.get_type_info("MyStruct").await;
        assert!(found.is_some(), "Should find by simple name");
        assert_eq!(found.unwrap().name, "crate::deeply::nested::MyStruct");
    }
}
