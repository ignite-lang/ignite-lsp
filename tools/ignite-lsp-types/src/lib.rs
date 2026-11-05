//! `ignite-lsp-types`: writes the Ignite module of LSP message types from the LSP meta model
//! (`docs/lsp/metaModel.json`). The types are generated, never written by hand
//! (`design:ignite-lsp`, "Decision 2026-11-05: generated types").
//!
//! The module holds every structure, enumeration and type alias reachable from the params and
//! results of the root methods. Mapping:
//!
//! - structure: a record, `extends` then `mixins` then its own properties flattened in that
//!   order (a redeclared property keeps its first position and takes the last type); an
//!   optional property is an `Option` defaulting to `None`; an optional property of an
//!   enumeration without custom values decodes a value it does not know as `None`, and a list
//!   of one skips such a value (LSP 3.17, "Enumerations"); a required one is refused;
//! - enumeration: an enum, one variant per value, encoded as its wire value (string or
//!   integer); an enumeration with `supportsCustomValues` gets a `Custom(Text)` or
//!   `Custom(Int)` variant that keeps any other value;
//! - `T | null`: `Option<T>`; an optional property of that type is `Option<Option<T>>`, so
//!   an absent member and `null` stay apart;
//! - any other `or`: an enum with one variant per arm, decoded by shape (see [`SHAPE_ORDER`]);
//! - base types: `string`, `DocumentUri`, `URI` are `Text`; `integer` and `uinteger` are `Int`
//!   (decoding checks their 32-bit ranges); `boolean` is `Bool`; `decimal` is `Json` (a
//!   `Number` or a `Decimal`); `null` is `Unit`;
//! - `LSPAny`, `LSPObject`, `LSPArray`: aliases of `Json`;
//! - `array`: `List`; `map`: `Map<Text, V>` (a key other than `string`, `DocumentUri` or
//!   `URI` is refused); `stringLiteral`: `Text`, decoding checks the literal; `literal`: a
//!   record named after the property that holds it.
//! - Field names are the wire names in `lower_snake`, with `_` appended to a keyword of Ignite
//!   or Rust (`type` is `type_`).
//!
//! Every record, enum and union has `to_json` and `from_json` over `std.json`; decoding
//! ignores members it does not know. A type the generator cannot map (`and`, `tuple`, an
//! unknown base type or reference, a map key that is not text) is refused with its name.

pub mod json;
pub mod model;

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;

use model::{Enumeration, MetaModel, Property, Type, Value};

/// The order in which a union's arms are tried when decoding: by class, and within a class
/// in the meta model's order. A structure arm with more required properties is tried before
/// one with fewer, then more properties before fewer, so a structure that extends another is
/// tried before its base. The first arm the value selects decodes it, and that decode's
/// result is the union's: see [`SELECTION_RULE`].
pub const SHAPE_ORDER: &[&str] = &[
    "string literals",
    "enumerations without custom values",
    "boolean",
    "integer, uinteger",
    "decimal",
    "string, DocumentUri, URI, enumerations with custom values",
    "arrays",
    "structures and literals (most required properties first, then most properties)",
    "maps",
    "unions",
    "LSPAny, LSPObject, LSPArray",
];

/// How decoding commits to a union arm, written into the module header. A structure arm whose
/// decode fails must not fall through to a looser arm: an incremental text change with a bad
/// range would otherwise decode as a full replacement.
pub const SELECTION_RULE: &[&str] = &[
    "Decoding commits to the first arm, in that order, that the value selects: a",
    "structure or literal arm when the value is an object with a member for every",
    "required property (null counts as present); a string literal arm when the value",
    "is that string; a union arm when the value selects one of its arms; any other",
    "arm when the value has its JSON kind (string, number, boolean, array, object).",
    "The selected arm's decode error is the union's error; a later arm is tried only",
    "when the value does not select an earlier one.",
];

/// Why no module was written.
#[derive(Debug, PartialEq)]
pub enum Error {
    /// The meta model is not JSON of the expected shape.
    Malformed(String),
    /// A reachable type has no mapping; the message names it.
    Refused(String),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Malformed(message) => write!(f, "malformed meta model: {message}"),
            Error::Refused(message) => write!(f, "refused: {message}"),
        }
    }
}

const JSON_ALIASES: &[&str] = &["LSPAny", "LSPObject", "LSPArray"];

const KEYWORDS: &[&str] = &[
    "package",
    "import",
    "type",
    "enum",
    "fn",
    "const",
    "protocol",
    "test",
    "system",
    "resource",
    "component",
    "contract",
    "policy",
    "assembly",
    "let",
    "var",
    "if",
    "else",
    "match",
    "while",
    "loop",
    "for",
    "in",
    "break",
    "continue",
    "return",
    "requires",
    "spawn",
    "assert",
    "true",
    "false",
    "await",
    "as",
    "crate",
    "extern",
    "impl",
    "mod",
    "move",
    "mut",
    "pub",
    "ref",
    "self",
    "static",
    "struct",
    "super",
    "trait",
    "unsafe",
    "use",
    "where",
    "async",
    "dyn",
    "abstract",
    "become",
    "box",
    "do",
    "final",
    "macro",
    "override",
    "priv",
    "typeof",
    "unsized",
    "virtual",
    "yield",
    "try",
    "gen",
];

#[derive(Clone, Debug, PartialEq)]
enum Ir {
    Text,
    Int,
    UInt,
    Bool,
    Unit,
    Json,
    Decimal,
    Literal(String),
    List(Box<Ir>),
    Map(Box<Ir>),
    Option(Box<Ir>),
    Named(String),
    Alias(String, Box<Ir>),
}

#[derive(Clone, Debug)]
struct Field {
    wire: String,
    ident: String,
    ir: Ir,
    optional: bool,
}

#[derive(Clone, Debug)]
struct Arm {
    variant: String,
    ir: Ir,
}

#[derive(Clone, Debug)]
enum Item {
    Pending(String),
    Record(String, Vec<Field>),
    Enum(String, Enumeration),
    Union(String, Vec<Arm>),
    Alias(String, Ir),
}

impl Item {
    fn origin(&self) -> &str {
        match self {
            Item::Pending(o)
            | Item::Record(o, _)
            | Item::Enum(o, _)
            | Item::Union(o, _)
            | Item::Alias(o, _) => o,
        }
    }
}

struct Gen<'a> {
    model: &'a MetaModel,
    items: BTreeMap<String, Item>,
    helpers: BTreeMap<String, String>,
    bases: BTreeSet<&'static str>,
    /// The type aliases being lowered, each with the `depth` it was entered at.
    open_aliases: BTreeMap<String, usize>,
    /// How many structures, literals and `or` arms enclose the type being lowered.
    depth: usize,
}

fn refuse<T>(message: String) -> Result<T, Error> {
    Err(Error::Refused(message))
}

/// `UpperCamel` of a meta model name: a leading `_` dropped, any other character that is
/// not a letter or digit dropped with the next letter capitalised.
fn pascal(name: &str) -> String {
    let mut out = String::new();
    let mut upper = true;
    for c in name.chars() {
        if c.is_ascii_alphanumeric() {
            if upper {
                out.push(c.to_ascii_uppercase());
            } else {
                out.push(c);
            }
            upper = false;
        } else {
            upper = true;
        }
    }
    out
}

/// `lower_snake` of a camelCase name; a run of capitals is one word (`documentURI` is
/// `document_uri`).
fn snake(name: &str) -> String {
    let chars: Vec<char> = name.chars().collect();
    let mut out = String::new();
    for (i, &c) in chars.iter().enumerate() {
        if c.is_ascii_uppercase() {
            let prev = if i > 0 { Some(chars[i - 1]) } else { None };
            let next = chars.get(i + 1).copied();
            let boundary = match prev {
                None => false,
                Some('_') => false,
                Some(p) if p.is_ascii_lowercase() || p.is_ascii_digit() => true,
                Some(p) => p.is_ascii_uppercase() && next.is_some_and(|n| n.is_ascii_lowercase()),
            };
            if boundary {
                out.push('_');
            }
            out.push(c.to_ascii_lowercase());
        } else if c.is_ascii_alphanumeric() || c == '_' {
            out.push(c);
        } else {
            out.push('_');
        }
    }
    out.trim_start_matches('_').to_string()
}

fn ident(name: &str) -> String {
    let base = snake(name);
    if KEYWORDS.contains(&base.as_str()) {
        format!("{base}_")
    } else {
        base
    }
}

/// An Ignite string literal of `text`.
fn lit(text: &str) -> String {
    let mut out = String::from("\"");
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '{' => out.push_str("{{"),
            '}' => out.push_str("}}"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                let _ = write!(out, "\\u{{{:x}}}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn base_name(ty: &Type) -> &'static str {
    match ty {
        Type::Base(_) => "base",
        Type::Reference(_) => "reference",
        Type::Array(_) => "array",
        Type::Map(..) => "map",
        Type::Or(_) => "or",
        Type::Literal(_) => "literal",
        Type::StringLiteral(_) => "stringLiteral",
        Type::Unknown(_) => "unknown",
    }
}

impl<'a> Gen<'a> {
    fn register(&mut self, name: &str, item: Item) -> Result<(), Error> {
        if let Some(existing) = self.items.get(name)
            && !matches!(existing, Item::Pending(_))
            && existing.origin() != item.origin()
        {
            return refuse(format!(
                "`{name}` would name both {} and {}",
                existing.origin(),
                item.origin()
            ));
        }
        self.items.insert(name.to_string(), item);
        Ok(())
    }

    /// The Ignite type of `ty`; `hint` names an anonymous union or literal, `origin` says
    /// where `ty` stands in the meta model.
    fn lower(&mut self, ty: &Type, hint: &str, origin: &str) -> Result<Ir, Error> {
        match ty {
            Type::Base(name) => match name.as_str() {
                "string" | "DocumentUri" | "URI" => Ok(Ir::Text),
                "integer" => Ok(Ir::Int),
                "uinteger" => Ok(Ir::UInt),
                "boolean" => Ok(Ir::Bool),
                "decimal" => Ok(Ir::Decimal),
                "null" => Ok(Ir::Unit),
                other => refuse(format!("base type `{other}` of {origin}")),
            },
            Type::Reference(name) => self.reference(name, origin),
            Type::Array(element) => Ok(Ir::List(Box::new(self.lower(
                element,
                &format!("{hint}Item"),
                origin,
            )?))),
            Type::Map(key, value) => {
                let key_ir = self.lower(key, hint, origin)?;
                let text_key = match &key_ir {
                    Ir::Text => true,
                    Ir::Alias(_, target) => **target == Ir::Text,
                    _ => false,
                };
                if !text_key {
                    return refuse(format!(
                        "map key of {origin}: only string, DocumentUri and URI keys map to `Map<Text, V>`"
                    ));
                }
                Ok(Ir::Map(Box::new(self.lower(
                    value,
                    &format!("{hint}Value"),
                    origin,
                )?)))
            }
            Type::Or(arms) => self.union(arms, hint, origin),
            Type::Literal(properties) => {
                let name = pascal(hint);
                let origin = format!("{origin} (literal)");
                if let Some(existing) = self.items.get(&name)
                    && existing.origin() == origin
                {
                    return Ok(Ir::Named(name));
                }
                self.register(&name, Item::Pending(origin.clone()))?;
                self.depth += 1;
                let fields = self.fields(&name, properties, &name, &origin)?;
                self.depth -= 1;
                self.register(&name, Item::Record(origin, fields))?;
                Ok(Ir::Named(name))
            }
            Type::StringLiteral(text) => Ok(Ir::Literal(text.clone())),
            Type::Unknown(kind) => refuse(format!("type kind `{kind}` of {origin}")),
        }
    }

    fn union(&mut self, arms: &[Type], hint: &str, origin: &str) -> Result<Ir, Error> {
        let nullable = arms
            .iter()
            .any(|arm| matches!(arm, Type::Base(name) if name == "null"));
        let others: Vec<&Type> = arms
            .iter()
            .filter(|arm| !matches!(arm, Type::Base(name) if name == "null"))
            .collect();
        let inner = match others.as_slice() {
            [] => return refuse(format!("`or` of {origin} has no arm but null")),
            [one] => self.lower(one, hint, origin)?,
            _ => {
                let name = pascal(hint);
                let origin = format!("{origin} (or)");
                if let Some(existing) = self.items.get(&name)
                    && existing.origin() == origin
                {
                    return Ok(wrap(Ir::Named(name), nullable));
                }
                self.register(&name, Item::Pending(origin.clone()))?;
                let mut lowered = Vec::new();
                self.depth += 1;
                for (index, arm) in others.iter().enumerate() {
                    let arm_hint = format!("{name}Literal{}", index + 1);
                    let arm_origin = format!("{origin} arm {}", index + 1);
                    let ir = self.lower(arm, &arm_hint, &arm_origin)?;
                    let variant = variant(arm, &ir);
                    if lowered.iter().any(|a: &Arm| a.variant == variant) {
                        return refuse(format!("two arms of {origin} would both be `{variant}`"));
                    }
                    if matches!(ir, Ir::Option(_) | Ir::Unit) {
                        return refuse(format!(
                            "arm {} of {origin} ({}) is nullable inside a union",
                            index + 1,
                            base_name(arm)
                        ));
                    }
                    lowered.push(Arm { variant, ir });
                }
                self.depth -= 1;
                self.register(&name, Item::Union(origin, lowered))?;
                Ir::Named(name)
            }
        };
        Ok(wrap(inner, nullable))
    }

    fn reference(&mut self, name: &str, origin: &str) -> Result<Ir, Error> {
        if let Some(item) = self.items.get(name) {
            return Ok(match item {
                Item::Alias(_, target) => Ir::Alias(name.to_string(), Box::new(target.clone())),
                _ => Ir::Named(name.to_string()),
            });
        }
        if JSON_ALIASES.contains(&name) {
            let origin = format!("typeAliases/{name}");
            self.register(name, Item::Alias(origin, Ir::Json))?;
            return Ok(Ir::Alias(name.to_string(), Box::new(Ir::Json)));
        }
        if !name.starts_with(|c: char| c.is_ascii_uppercase()) {
            return refuse(format!(
                "reference to `{name}` from {origin}: an Ignite type name starts with a capital"
            ));
        }
        let model = self.model;
        if model.structures.iter().any(|s| s.name == name) {
            let origin = format!("structures/{name}");
            self.register(name, Item::Pending(origin.clone()))?;
            let properties = self.flatten(name, &mut Vec::new())?;
            self.depth += 1;
            let mut fields = Vec::new();
            let mut seen = BTreeSet::new();
            for (owner, property) in properties {
                let owner_hint = pascal(&owner);
                let field =
                    self.field(name, &owner_hint, &property, &format!("structures/{owner}"))?;
                if !seen.insert(field.ident.clone()) {
                    return refuse(format!(
                        "two properties of `{name}` would both be `{}`",
                        field.ident
                    ));
                }
                fields.push(field);
            }
            self.depth -= 1;
            self.register(name, Item::Record(origin, fields))?;
            return Ok(Ir::Named(name.to_string()));
        }
        if let Some(enumeration) = model.enumerations.iter().find(|e| e.name == name) {
            self.enumeration(enumeration)?;
            return Ok(Ir::Named(name.to_string()));
        }
        if let Some(alias) = model.aliases.iter().find(|a| a.name == name) {
            let from = origin;
            let origin = format!("typeAliases/{name}");
            if let Some(&entered) = self.open_aliases.get(name) {
                if entered == self.depth {
                    return refuse(format!(
                        "type alias `{name}` contains itself with no structure or `or` between (reached again from {from})"
                    ));
                }
                let ir = self.lower(&alias.ty, name, &origin)?;
                return Ok(if ir == Ir::Named(name.to_string()) {
                    ir
                } else {
                    Ir::Alias(name.to_string(), Box::new(ir))
                });
            }
            self.open_aliases.insert(name.to_string(), self.depth);
            let lowered = self.lower(&alias.ty, name, &origin);
            self.open_aliases.remove(name);
            let ir = lowered?;
            if ir == Ir::Named(name.to_string()) {
                return Ok(ir);
            }
            self.register(name, Item::Alias(origin, ir.clone()))?;
            return Ok(Ir::Alias(name.to_string(), Box::new(ir)));
        }
        refuse(format!("reference to unknown type `{name}` from {origin}"))
    }

    fn enumeration(&mut self, enumeration: &Enumeration) -> Result<(), Error> {
        let name = &enumeration.name;
        let origin = format!("enumerations/{name}");
        if !matches!(enumeration.base.as_str(), "string" | "integer" | "uinteger") {
            return refuse(format!(
                "enumeration `{name}` of base type `{}`",
                enumeration.base
            ));
        }
        if enumeration.values.is_empty() {
            return refuse(format!("enumeration `{name}` has no values"));
        }
        let mut seen = BTreeSet::new();
        for (value_name, value) in &enumeration.values {
            let variant = pascal(value_name);
            if variant.is_empty() || !seen.insert(variant.clone()) {
                return refuse(format!(
                    "enumeration `{name}`: value `{value_name}` has no distinct variant name"
                ));
            }
            let text = matches!(value, Value::Text(_));
            if text != (enumeration.base == "string") {
                return refuse(format!(
                    "enumeration `{name}`: value `{value_name}` does not have its base type"
                ));
            }
        }
        if enumeration.custom && seen.contains("Custom") {
            return refuse(format!(
                "enumeration `{name}` has a value named Custom and custom values"
            ));
        }
        self.register(name, Item::Enum(origin, enumeration.clone()))
    }

    /// The properties of structure `name` with `extends` and `mixins` flattened, each with the
    /// structure that declares it.
    fn flatten(
        &self,
        name: &str,
        path: &mut Vec<String>,
    ) -> Result<Vec<(String, Property)>, Error> {
        if path.iter().any(|p| p == name) {
            return refuse(format!("structure `{name}` extends itself"));
        }
        let Some(structure) = self.model.structures.iter().find(|s| s.name == name) else {
            return refuse(format!(
                "`{}` extends or mixes in `{name}`, which is no structure",
                path.last().map_or("?", |s| s)
            ));
        };
        path.push(name.to_string());
        let mut out: Vec<(String, Property)> = Vec::new();
        for parent in structure.extends.iter().chain(structure.mixins.iter()) {
            let Type::Reference(parent) = parent else {
                return refuse(format!("`{name}` extends a {} type", base_name(parent)));
            };
            for entry in self.flatten(parent, path)? {
                merge(&mut out, entry);
            }
        }
        for property in &structure.properties {
            merge(&mut out, (name.to_string(), property.clone()));
        }
        path.pop();
        Ok(out)
    }

    fn field(
        &mut self,
        record: &str,
        owner_hint: &str,
        property: &Property,
        owner_origin: &str,
    ) -> Result<Field, Error> {
        let origin = format!("{owner_origin}.{}", property.name);
        let hint = format!("{owner_hint}{}", pascal(&property.name));
        let ir = self.lower(&property.ty, &hint, &origin)?;
        let ident = ident(&property.name);
        if ident.is_empty() || !ident.starts_with(|c: char| c.is_ascii_lowercase()) {
            return refuse(format!(
                "property `{}` of `{record}` has no Ignite field name",
                property.name
            ));
        }
        Ok(Field {
            wire: property.name.clone(),
            ident,
            ir,
            optional: property.optional,
        })
    }

    fn fields(
        &mut self,
        record: &str,
        properties: &[Property],
        hint: &str,
        origin: &str,
    ) -> Result<Vec<Field>, Error> {
        let mut fields = Vec::new();
        let mut seen = BTreeSet::new();
        for property in properties {
            let field = self.field(
                record,
                hint,
                property,
                origin.trim_end_matches(" (literal)"),
            )?;
            if !seen.insert(field.ident.clone()) {
                return refuse(format!(
                    "two properties of `{record}` would both be `{}`",
                    field.ident
                ));
            }
            fields.push(field);
        }
        Ok(fields)
    }

    // ---- emission ----------------------------------------------------------------------

    fn ty_text(&self, ir: &Ir) -> String {
        match ir {
            Ir::Text | Ir::Literal(_) => "Text".into(),
            Ir::Int | Ir::UInt => "Int".into(),
            Ir::Bool => "Bool".into(),
            Ir::Unit => "Unit".into(),
            Ir::Json | Ir::Decimal => "Json".into(),
            Ir::List(x) => format!("List<{}>", self.ty_text(x)),
            Ir::Map(x) => format!("Map<Text, {}>", self.ty_text(x)),
            Ir::Option(x) => format!("Option<{}>", self.ty_text(x)),
            Ir::Named(n) | Ir::Alias(n, _) => n.clone(),
        }
    }

    fn key(&self, ir: &Ir) -> String {
        match ir {
            Ir::Text => "text".into(),
            Ir::Int => "int".into(),
            Ir::UInt => "uinteger".into(),
            Ir::Bool => "bool".into(),
            Ir::Unit => "null".into(),
            Ir::Json => "json".into(),
            Ir::Decimal => "decimal".into(),
            Ir::Literal(text) => format!("literal_{}", snake(text)),
            Ir::List(x) => format!("list_of_{}", self.key(x)),
            Ir::Map(x) => format!("map_of_{}", self.key(x)),
            Ir::Option(x) => format!("option_of_{}", self.key(x)),
            Ir::Named(n) => snake(n),
            Ir::Alias(_, target) => self.key(target),
        }
    }

    /// An expression of type `Result<T, Text>` decoding the `Json` expression `value`; `at`
    /// is a `Text` expression naming the place for an error.
    fn decode(&mut self, ir: &Ir, value: &str, at: &str) -> String {
        let base = |emitter: &mut Self, name: &'static str| {
            emitter.bases.insert(name);
            format!("{name}({value}, {at})")
        };
        match ir {
            Ir::Text => base(self, "decode_text"),
            Ir::Int => base(self, "decode_int"),
            Ir::UInt => base(self, "decode_uinteger"),
            Ir::Bool => base(self, "decode_bool"),
            Ir::Unit => base(self, "decode_null"),
            Ir::Decimal => base(self, "decode_decimal"),
            Ir::Json => base(self, "decode_json"),
            Ir::Literal(text) => {
                self.bases.insert("decode_literal");
                format!("decode_literal({value}, {}, {at})", lit(text))
            }
            Ir::List(_) | Ir::Map(_) | Ir::Option(_) => {
                let key = self.key(ir);
                self.helper(ir);
                format!("decode_{key}({value}, {at})")
            }
            Ir::Named(n) => format!("{n}.from_json({value}).map_err(|e| {at} + \": \" + e)"),
            Ir::Alias(_, target) => self.decode(&target.clone(), value, at),
        }
    }

    /// An expression of type `Json` encoding the expression `value`.
    fn encode(&mut self, ir: &Ir, value: &str) -> String {
        match ir {
            Ir::Text | Ir::Literal(_) => format!("Json.Str({value})"),
            Ir::Int | Ir::UInt => format!("Json.Number({value})"),
            Ir::Bool => format!("Json.Bool({value})"),
            Ir::Unit => "Json.Null".into(),
            Ir::Json | Ir::Decimal => value.to_string(),
            Ir::List(_) | Ir::Map(_) | Ir::Option(_) => {
                let key = self.key(ir);
                self.helper(ir);
                format!("encode_{key}({value})")
            }
            Ir::Named(n) => format!("{n}.to_json({value})"),
            Ir::Alias(_, target) => self.encode(&target.clone(), value),
        }
    }

    /// Whether `ir` names an enumeration without custom values.
    fn closed_enumeration(&self, ir: &Ir) -> bool {
        match ir {
            Ir::Named(name) => matches!(self.items.get(name), Some(Item::Enum(_, e)) if !e.custom),
            _ => false,
        }
    }

    fn helper(&mut self, ir: &Ir) {
        let key = self.key(ir);
        if self.helpers.contains_key(&key) {
            return;
        }
        self.helpers.insert(key.clone(), String::new());
        let t = self.ty_text(ir);
        let mut out = String::new();
        match ir {
            Ir::List(x) if self.closed_enumeration(x) => {
                let e = self.ty_text(x);
                let encode = self.encode(x, "item");
                let _ = write!(
                    out,
                    "// A JSON array of {e}; an item that is none of its values is skipped (LSP 3.17,\n\
                     // \"Enumerations\": the using side \"shouldn't fail on an enumeration value it\n\
                     // doesn't know\"), so a client capability's `valueSet` that names a newer value\n\
                     // still decodes.\n\
                     fn decode_{key}(value: Json, at: Text) -> Result<{t}, Text> = match value {{\n  \
                       Json.Array(items) => {{\n    \
                         var out: {t} = []\n    \
                         for item in items {{\n      \
                           match {e}.from_json(item) {{\n        \
                             Ok(known) => out.push(known)\n        \
                             Err(_) => {{}}\n      \
                           }}\n    \
                         }}\n    \
                         Ok(out)\n  \
                       }}\n  \
                       _ => Err(at + \": expected an array\")\n\
                     }}\n\n\
                     fn encode_{key}(items: {t}) -> Json = Json.Array(items.map(|item| {encode}))\n"
                );
            }
            Ir::List(x) => {
                let e = self.ty_text(x);
                let decode = self.decode(x, "item", "\"{at}[{index}]\"");
                let encode = self.encode(x, "item");
                let _ = write!(
                    out,
                    "// A JSON array of {e}.\n\
                     fn decode_{key}(value: Json, at: Text) -> Result<{t}, Text> = match value {{\n  \
                       Json.Array(items) => {{\n    \
                         var out: {t} = []\n    \
                         var index = 0\n    \
                         for item in items {{\n      \
                           out.push({decode}?)\n      \
                           index = index + 1\n    \
                         }}\n    \
                         Ok(out)\n  \
                       }}\n  \
                       _ => Err(at + \": expected an array\")\n\
                     }}\n\n\
                     fn encode_{key}(items: {t}) -> Json = Json.Array(items.map(|item| {encode}))\n"
                );
            }
            Ir::Map(x) => {
                let e = self.ty_text(x);
                let decode = self.decode(x, "member.1", "at + \".\" + member.0");
                let encode = self.encode(x, "entry");
                let _ = write!(
                    out,
                    "// A JSON object whose member values are {e}, keyed by member name.\n\
                     fn decode_{key}(value: Json, at: Text) -> Result<{t}, Text> = match value {{\n  \
                       Json.Object(members) => {{\n    \
                         var out: {t} = Map.empty()\n    \
                         for member in members {{\n      \
                           out.put(member.0, {decode}?)\n    \
                         }}\n    \
                         Ok(out)\n  \
                       }}\n  \
                       _ => Err(at + \": expected an object\")\n\
                     }}\n\n\
                     fn encode_{key}(entries: {t}) -> Json {{\n  \
                       var members: List<(Text, Json)> = []\n  \
                       for (name, entry) in entries {{\n    \
                         members.push((name, {encode}))\n  \
                       }}\n  \
                       Json.Object(members)\n\
                     }}\n"
                );
            }
            Ir::Option(x) => {
                let e = self.ty_text(x);
                let decode = self.decode(x, "other", "at");
                let encode = self.encode(x, "item");
                let _ = write!(
                    out,
                    "// {e} or null.\n\
                     fn decode_{key}(value: Json, at: Text) -> Result<{t}, Text> = match value {{\n  \
                       Json.Null => Ok(Option.None)\n  \
                       other => Ok(Some({decode}?))\n\
                     }}\n\n\
                     fn encode_{key}(value: {t}) -> Json = match value {{\n  \
                       Some(item) => {encode}\n  \
                       Option.None => Json.Null\n\
                     }}\n"
                );
            }
            _ => unreachable!("helpers exist for lists, maps and options"),
        }
        self.helpers.insert(key, out);
    }

    /// The decoding class of an arm and its order inside the class (see [`SHAPE_ORDER`]).
    fn rank(&self, ir: &Ir) -> (u8, i64, i64) {
        match ir {
            Ir::Literal(_) => (0, 0, 0),
            Ir::Bool => (2, 0, 0),
            Ir::Int | Ir::UInt => (3, 0, 0),
            Ir::Decimal => (4, 0, 0),
            Ir::Text => (5, 0, 0),
            Ir::List(_) => (6, 0, 0),
            Ir::Map(_) => (8, 0, 0),
            Ir::Json => (10, 0, 0),
            Ir::Unit | Ir::Option(_) => (11, 0, 0),
            Ir::Alias(_, target) => self.rank(target),
            Ir::Named(n) => match self.items.get(n) {
                Some(Item::Enum(_, e)) => (if e.custom { 5 } else { 1 }, 0, 0),
                Some(Item::Record(_, fields)) => {
                    let required = fields.iter().filter(|f| !f.optional).count() as i64;
                    (7, -required, -(fields.len() as i64))
                }
                Some(Item::Union(..)) => (9, 0, 0),
                _ => (11, 0, 0),
            },
        }
    }

    fn emit_record(&mut self, name: &str, origin: &str, fields: &[Field]) -> String {
        let mut out = String::new();
        let _ = writeln!(out, "// metaModel: {origin}");
        if fields.is_empty() {
            let _ = writeln!(out, "type {name} {{}}");
        } else {
            let _ = writeln!(out, "type {name} {{");
            for field in fields {
                let mut t = self.ty_text(&field.ir);
                if field.optional {
                    t = format!("Option<{t}> = Option.None");
                }
                let _ = writeln!(out, "  {}: {t}", field.ident);
            }
            let _ = writeln!(out, "}}");
        }
        let _ = writeln!(out);
        let _ = writeln!(
            out,
            "// The JSON object of a `{name}`: members in field order, an absent option left out."
        );
        let _ = writeln!(out, "fn {name}.to_json(value: {name}) -> Json {{");
        let _ = writeln!(out, "  var members: List<(Text, Json)> = []");
        for field in fields {
            let wire = lit(&field.wire);
            if field.optional {
                let encode = self.encode(&field.ir, "item");
                let _ = writeln!(out, "  match value.{} {{", field.ident);
                let _ = writeln!(out, "    Some(item) => members.push(({wire}, {encode}))");
                let _ = writeln!(out, "    Option.None => {{}}");
                let _ = writeln!(out, "  }}");
            } else {
                let encode = self.encode(&field.ir, &format!("value.{}", field.ident));
                let _ = writeln!(out, "  members.push(({wire}, {encode}))");
            }
        }
        let _ = writeln!(out, "  Json.Object(members)");
        let _ = writeln!(out, "}}");
        let _ = writeln!(out);
        let _ = writeln!(
            out,
            "// The `{name}` of a JSON object; members it does not know are ignored."
        );
        let _ = writeln!(
            out,
            "fn {name}.from_json(value: Json) -> Result<{name}, Text> {{"
        );
        let _ = writeln!(out, "  match value {{");
        let _ = writeln!(out, "    Json.Object(_) => {{}}");
        let _ = writeln!(
            out,
            "    _ => return Err({})",
            lit(&format!("{name}: expected an object"))
        );
        let _ = writeln!(out, "  }}");
        for field in fields {
            let at = lit(&format!("{name}.{}", field.wire));
            let decode = self.decode(&field.ir, "member", &at);
            let _ = writeln!(
                out,
                "  let f_{} = match value.get({}) {{",
                field.ident,
                lit(&field.wire)
            );
            if field.optional && self.closed_enumeration(&field.ir) {
                let e = self.ty_text(&field.ir);
                let _ = writeln!(out, "    Some(member) => match {e}.from_json(member) {{");
                let _ = writeln!(out, "      Ok(known) => Some(known)");
                let _ = writeln!(out, "      Err(_) => Option.None");
                let _ = writeln!(out, "    }}");
                let _ = writeln!(out, "    Option.None => Option.None");
            } else if field.optional {
                let _ = writeln!(out, "    Some(member) => Some({decode}?)");
                let _ = writeln!(out, "    Option.None => Option.None");
            } else {
                let _ = writeln!(out, "    Some(member) => {decode}?");
                let _ = writeln!(
                    out,
                    "    Option.None => return Err({})",
                    lit(&format!("{name}.{}: missing", field.wire))
                );
            }
            let _ = writeln!(out, "  }}");
        }
        let assignments: Vec<String> = fields
            .iter()
            .map(|f| format!("{} = f_{}", f.ident, f.ident))
            .collect();
        if assignments.is_empty() {
            let _ = writeln!(out, "  Ok({name} {{}})");
        } else {
            let _ = writeln!(out, "  Ok({name} {{ {} }})", assignments.join(", "));
        }
        let _ = writeln!(out, "}}");
        out
    }

    fn emit_enum(&self, name: &str, origin: &str, e: &Enumeration) -> String {
        let mut out = String::new();
        let custom_ty = if e.base == "string" { "Text" } else { "Int" };
        let note = if e.custom {
            format!("; other values kept as `Custom({custom_ty})`")
        } else {
            String::new()
        };
        let _ = writeln!(out, "// metaModel: {origin} ({}{note})", e.base);
        let mut variants: Vec<String> = e.values.iter().map(|(n, _)| pascal(n)).collect();
        if e.custom {
            variants.push(format!("Custom({custom_ty})"));
        }
        let _ = writeln!(out, "enum {name} {{");
        for (i, variant) in variants.iter().enumerate() {
            let comma = if i + 1 < variants.len() { "," } else { "" };
            let _ = writeln!(out, "  {variant}{comma}");
        }
        let _ = writeln!(out, "}}");
        let _ = writeln!(out);
        let wire = |value: &Value| match value {
            Value::Text(text) => format!("Json.Str({})", lit(text)),
            Value::Int(n) => format!("Json.Number({n})"),
        };
        let _ = writeln!(out, "// The wire value of a `{name}`.");
        let _ = writeln!(
            out,
            "fn {name}.to_json(value: {name}) -> Json = match value {{"
        );
        for (value_name, value) in &e.values {
            let _ = writeln!(out, "  {name}.{} => {}", pascal(value_name), wire(value));
        }
        if e.custom {
            let ctor = if e.base == "string" {
                "Json.Str"
            } else {
                "Json.Number"
            };
            let _ = writeln!(out, "  {name}.Custom(other) => {ctor}(other)");
        }
        let _ = writeln!(out, "}}");
        let _ = writeln!(out);
        let _ = writeln!(out, "// The `{name}` of a wire value.");
        let _ = writeln!(
            out,
            "fn {name}.from_json(value: Json) -> Result<{name}, Text> = match value {{"
        );
        for (value_name, value) in &e.values {
            let _ = writeln!(
                out,
                "  {} => Ok({name}.{})",
                wire(value),
                pascal(value_name)
            );
        }
        if e.custom {
            let ctor = if e.base == "string" {
                "Json.Str"
            } else {
                "Json.Number"
            };
            let _ = writeln!(out, "  {ctor}(other) => Ok({name}.Custom(other))");
        }
        let _ = writeln!(
            out,
            "  _ => Err({})",
            lit(&format!("{name}: not one of its values"))
        );
        let _ = writeln!(out, "}}");
        out
    }

    fn emit_union(&mut self, name: &str, origin: &str, arms: &[Arm]) -> String {
        let mut order: Vec<(usize, &Arm)> = arms.iter().enumerate().collect();
        order.sort_by_key(|(index, arm)| (self.rank(&arm.ir), *index));
        let mut out = String::new();
        let _ = writeln!(out, "// metaModel: {origin}");
        let _ = writeln!(out, "enum {name} {{");
        for (i, arm) in arms.iter().enumerate() {
            let comma = if i + 1 < arms.len() { "," } else { "" };
            let _ = writeln!(out, "  {}({}){comma}", arm.variant, self.ty_text(&arm.ir));
        }
        let _ = writeln!(out, "}}");
        let _ = writeln!(out);
        let _ = writeln!(out, "// The JSON of the arm a `{name}` holds.");
        let _ = writeln!(
            out,
            "fn {name}.to_json(value: {name}) -> Json = match value {{"
        );
        for arm in arms {
            let encode = self.encode(&arm.ir, "arm");
            let _ = writeln!(out, "  {name}.{}(arm) => {encode}", arm.variant);
        }
        let _ = writeln!(out, "}}");
        let _ = writeln!(out);
        let names: Vec<&str> = order.iter().map(|(_, a)| a.variant.as_str()).collect();
        let selections: Vec<String> = order
            .iter()
            .map(|(_, arm)| self.selects(&arm.ir, "value"))
            .collect();
        let _ = writeln!(
            out,
            "// Whether a value selects an arm of `{name}`: the JSON kind of one, or every required member of a structure arm."
        );
        let _ = writeln!(
            out,
            "fn {name}.selects(value: Json) -> Bool = {}",
            selections.join(" || ")
        );
        let _ = writeln!(out);
        let _ = writeln!(
            out,
            "// Decoded by shape: the first arm the value selects decodes it, or its error is the union's: {}.",
            names.join(", then ")
        );
        let _ = writeln!(
            out,
            "fn {name}.from_json(value: Json) -> Result<{name}, Text> {{"
        );
        for ((_, arm), selection) in order.iter().zip(&selections) {
            let decode = self.decode(&arm.ir, "value", &lit(name));
            let _ = writeln!(out, "  if {selection} {{");
            let _ = writeln!(out, "    return match {decode} {{");
            let _ = writeln!(out, "      Ok(arm) => Ok({name}.{}(arm))", arm.variant);
            let _ = writeln!(out, "      Err(e) => Err(e)");
            let _ = writeln!(out, "    }}");
            let _ = writeln!(out, "  }}");
        }
        let _ = writeln!(
            out,
            "  Err({})",
            lit(&format!("{name}: matches none of {}", names.join(", ")))
        );
        let _ = writeln!(out, "}}");
        out
    }

    /// A `Bool` expression: whether the `Json` expression `value` selects an arm of type
    /// `ir`. A structure or literal arm is selected when the value is an object holding every
    /// required member; a string literal arm when the value is that string; a union arm when
    /// the value selects one of its arms; any other arm when the value has its JSON kind.
    fn selects(&mut self, ir: &Ir, value: &str) -> String {
        match ir {
            Ir::Text => self.predicate("is_string", value),
            Ir::Int | Ir::UInt | Ir::Decimal => self.predicate("is_number", value),
            Ir::Bool => self.predicate("is_bool", value),
            Ir::List(_) => self.predicate("is_array", value),
            Ir::Map(_) => self.predicate("is_object", value),
            Ir::Unit | Ir::Option(_) => self.predicate("is_null", value),
            Ir::Json => "true".into(),
            Ir::Literal(text) => format!("{value} == Json.Str({})", lit(text)),
            Ir::Alias(_, target) => self.selects(&target.clone(), value),
            Ir::Named(n) => match self.items.get(n).cloned() {
                Some(Item::Enum(_, e)) if e.base == "string" => self.predicate("is_string", value),
                Some(Item::Enum(..)) => self.predicate("is_number", value),
                Some(Item::Record(_, fields)) => {
                    let required: Vec<String> = fields
                        .iter()
                        .filter(|f| !f.optional)
                        .map(|f| lit(&f.wire))
                        .collect();
                    if required.is_empty() {
                        self.predicate("is_object", value)
                    } else {
                        self.bases.insert("has_members");
                        self.bases.insert("is_object");
                        format!("has_members({value}, [{}])", required.join(", "))
                    }
                }
                _ => format!("{n}.selects({value})"),
            },
        }
    }

    fn predicate(&mut self, name: &'static str, value: &str) -> String {
        self.bases.insert(name);
        format!("{name}({value})")
    }

    fn emit_base(name: &str) -> &'static str {
        match name {
            "decode_text" => {
                "// A JSON string.\nfn decode_text(value: Json, at: Text) -> Result<Text, Text> = match value {\n  Json.Str(text) => Ok(text)\n  _ => Err(at + \": expected a string\")\n}\n"
            }
            "decode_int" => {
                "// An LSP `integer`: -2^31 to 2^31 - 1.\nfn decode_int(value: Json, at: Text) -> Result<Int, Text> = match value {\n  Json.Number(n) => if n >= -2147483648 && n <= 2147483647 { Ok(n) } else { Err(at + \": integer out of range\") }\n  _ => Err(at + \": expected an integer\")\n}\n"
            }
            "decode_uinteger" => {
                "// An LSP `uinteger`: 0 to 2^31 - 1.\nfn decode_uinteger(value: Json, at: Text) -> Result<Int, Text> = match value {\n  Json.Number(n) => if n >= 0 && n <= 2147483647 { Ok(n) } else { Err(at + \": uinteger out of range\") }\n  _ => Err(at + \": expected an unsigned integer\")\n}\n"
            }
            "decode_bool" => {
                "// A JSON boolean.\nfn decode_bool(value: Json, at: Text) -> Result<Bool, Text> = match value {\n  Json.Bool(b) => Ok(b)\n  _ => Err(at + \": expected a boolean\")\n}\n"
            }
            "is_string" => {
                "// Whether a value is a JSON string.\nfn is_string(value: Json) -> Bool = match value {\n  Json.Str(_) => true\n  _ => false\n}\n"
            }
            "is_number" => {
                "// Whether a value is a JSON number.\nfn is_number(value: Json) -> Bool = match value {\n  Json.Number(_) => true\n  Json.Decimal(_) => true\n  _ => false\n}\n"
            }
            "is_bool" => {
                "// Whether a value is a JSON boolean.\nfn is_bool(value: Json) -> Bool = match value {\n  Json.Bool(_) => true\n  _ => false\n}\n"
            }
            "is_array" => {
                "// Whether a value is a JSON array.\nfn is_array(value: Json) -> Bool = match value {\n  Json.Array(_) => true\n  _ => false\n}\n"
            }
            "is_object" => {
                "// Whether a value is a JSON object.\nfn is_object(value: Json) -> Bool = match value {\n  Json.Object(_) => true\n  _ => false\n}\n"
            }
            "is_null" => {
                "// Whether a value is JSON null.\nfn is_null(value: Json) -> Bool = value == Json.Null\n"
            }
            "has_members" => {
                "// Whether a value is a JSON object with a member of each of `names`, null or not.\nfn has_members(value: Json, names: List<Text>) -> Bool {\n  if !is_object(value) {\n    return false\n  }\n  for name in names {\n    if value.get(name) == Option.None {\n      return false\n    }\n  }\n  true\n}\n"
            }
            "decode_json" => {
                "// Any JSON value (`LSPAny`, `LSPObject`, `LSPArray`); `at` is unused, every value is one.\nfn decode_json(value: Json, at: Text) -> Result<Json, Text> = Ok(value)\n"
            }
            "decode_null" => {
                "// JSON null.\nfn decode_null(value: Json, at: Text) -> Result<Unit, Text> = match value {\n  Json.Null => Ok(())\n  _ => Err(at + \": expected null\")\n}\n"
            }
            "decode_decimal" => {
                "// An LSP `decimal`: any JSON number, kept as its `Json`.\nfn decode_decimal(value: Json, at: Text) -> Result<Json, Text> = match value {\n  Json.Number(_) => Ok(value)\n  Json.Decimal(_) => Ok(value)\n  _ => Err(at + \": expected a number\")\n}\n"
            }
            "decode_literal" => {
                "// A JSON string equal to `literal`.\nfn decode_literal(value: Json, literal: Text, at: Text) -> Result<Text, Text> = match value {\n  Json.Str(text) => if text == literal { Ok(text) } else { Err(at + \": expected \" + literal) }\n  _ => Err(at + \": expected a string\")\n}\n"
            }
            _ => unreachable!("no base decoder {name}"),
        }
    }
}

/// `Option<ir>` when the `or` it came from had a `null` arm.
fn wrap(ir: Ir, nullable: bool) -> Ir {
    if nullable {
        Ir::Option(Box::new(ir))
    } else {
        ir
    }
}

fn merge(out: &mut Vec<(String, Property)>, entry: (String, Property)) {
    match out.iter_mut().find(|(_, p)| p.name == entry.1.name) {
        Some(slot) => *slot = entry,
        None => out.push(entry),
    }
}

fn variant(ty: &Type, ir: &Ir) -> String {
    match (ty, ir) {
        (Type::Reference(name), _) => pascal(name),
        (Type::Base(name), _) => pascal(name),
        (Type::Array(element), Ir::List(inner)) => format!("{}List", variant(element, inner)),
        (Type::Map(_, value), Ir::Map(inner)) => format!("{}Map", variant(value, inner)),
        (Type::StringLiteral(text), _) => pascal(text),
        (_, Ir::Named(name)) => name.clone(),
        _ => pascal(base_name(ty)),
    }
}

fn method_const(method: &str) -> String {
    let snake = snake(method);
    let mut out = String::from("METHOD");
    for part in snake.split('_').filter(|p| !p.is_empty()) {
        out.push('_');
        out.push_str(&part.to_ascii_uppercase());
    }
    out
}

/// The SHA-256 of `text`, lowercase hex.
pub fn sha256_hex(text: &str) -> String {
    use sha2::{Digest, Sha256};
    Sha256::digest(text.as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// The Ignite module `package` of every type reachable from `roots` of the meta model
/// `text`; its header names the meta model's version and SHA-256.
pub fn generate(text: &str, roots: &[String], package: &str) -> Result<String, Error> {
    generate_with_types(text, roots, &[], package)
}

/// [`generate`] with `types` as further roots: structures, enumerations or type aliases of
/// the meta model that no root method reaches, such as `ErrorCodes`.
pub fn generate_with_types(
    text: &str,
    roots: &[String],
    types: &[String],
    package: &str,
) -> Result<String, Error> {
    let sha256 = sha256_hex(text);
    let model = MetaModel::parse(text).map_err(Error::Malformed)?;
    let mut emitter = Gen {
        model: &model,
        items: BTreeMap::new(),
        helpers: BTreeMap::new(),
        bases: BTreeSet::new(),
        open_aliases: BTreeMap::new(),
        depth: 0,
    };
    let mut consts = String::new();
    for root in roots {
        let Some(message) = model.messages.iter().find(|m| &m.method == root) else {
            return refuse(format!(
                "root `{root}` is no request or notification of the meta model"
            ));
        };
        let kind = if message.request {
            "requests"
        } else {
            "notifications"
        };
        let origin = format!("{kind}/{root}");
        let hint = pascal(root);
        let mut parts = Vec::new();
        if let Some(params) = &message.params {
            let ir = emitter.lower(
                params,
                &format!("{hint}Params"),
                &format!("{origin} params"),
            )?;
            parts.push(format!("params {}", emitter.ty_text(&ir)));
        }
        if let Some(result) = &message.result {
            let ir = emitter.lower(
                result,
                &format!("{hint}Result"),
                &format!("{origin} result"),
            )?;
            parts.push(format!("result {}", emitter.ty_text(&ir)));
        }
        let detail = if parts.is_empty() {
            "no params".to_string()
        } else {
            parts.join(", ")
        };
        let _ = writeln!(consts, "// metaModel: {origin} ({detail})");
        let _ = writeln!(consts, "const {}: Text = {}", method_const(root), lit(root));
        let _ = writeln!(consts);
    }

    for name in types {
        let known = model.structures.iter().any(|s| &s.name == name)
            || model.enumerations.iter().any(|e| &e.name == name)
            || model.aliases.iter().any(|a| &a.name == name);
        if !known {
            return refuse(format!(
                "root type `{name}` is no structure, enumeration or type alias of the meta model"
            ));
        }
        emitter.lower(
            &Type::Reference(name.clone()),
            name,
            &format!("root type {name}"),
        )?;
    }

    let items: Vec<(String, Item)> = emitter
        .items
        .iter()
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    let mut counts = BTreeMap::new();
    let mut sections: BTreeMap<u8, Vec<String>> = BTreeMap::new();
    for (name, item) in &items {
        let (section, text) = match item {
            Item::Alias(origin, target) => {
                let mapped = if JSON_ALIASES.contains(&name.as_str()) {
                    " (mapped to Json)"
                } else {
                    ""
                };
                (
                    0,
                    format!(
                        "// metaModel: {origin}{mapped}\ntype {name} = {}\n",
                        emitter.ty_text(target)
                    ),
                )
            }
            Item::Enum(origin, e) => (1, emitter.emit_enum(name, origin, e)),
            Item::Record(origin, fields) => (2, emitter.emit_record(name, origin, fields)),
            Item::Union(origin, arms) => (3, emitter.emit_union(name, origin, arms)),
            Item::Pending(origin) => {
                return refuse(format!("`{name}` ({origin}) was never completed"));
            }
        };
        let kind = match (section, item) {
            (2, Item::Record(origin, _)) if origin.ends_with("(literal)") => "literal records",
            (0, _) => "type aliases",
            (1, _) => "enumerations",
            (2, _) => "structure records",
            _ => "unions",
        };
        *counts.entry(kind).or_insert(0usize) += 1;
        sections.entry(section).or_default().push(text);
    }

    let mut out = String::new();
    let _ = writeln!(out, "package {package}");
    let _ = writeln!(out);
    let _ = writeln!(out, "import std.json.{{Json, get}}");
    let _ = writeln!(out, "import std.result.map_err");
    let _ = writeln!(out);
    let _ = writeln!(
        out,
        "// Generated by tools/ignite-lsp-types (`task lsp-types`): do not edit."
    );
    let _ = writeln!(
        out,
        "// Source: metaModel.json of LSP {}, sha256 {}.",
        model.version, sha256
    );
    let _ = writeln!(out, "// Roots: {}.", roots.join(", "));
    if !types.is_empty() {
        let _ = writeln!(out, "// Root types: {}.", types.join(", "));
    }
    let summary: Vec<String> = counts.iter().map(|(k, v)| format!("{v} {k}")).collect();
    let _ = writeln!(out, "// Items: {}.", summary.join(", "));
    let _ = writeln!(out, "//");
    let _ = writeln!(
        out,
        "// Each item names the metaModel entry it comes from. A structure is a record with"
    );
    let _ = writeln!(
        out,
        "// `extends` and `mixins` flattened; an optional property is an `Option`, a"
    );
    let _ = writeln!(
        out,
        "// property of type `T | null` an `Option<T>`, and one that is both an"
    );
    let _ = writeln!(
        out,
        "// `Option<Option<T>>` (absent, null). Any other `or` is an enum decoded by"
    );
    let _ = writeln!(out, "// shape: its arms are tried in the order");
    for (i, class) in SHAPE_ORDER.iter().enumerate() {
        let end = if i + 1 == SHAPE_ORDER.len() { "." } else { ";" };
        let _ = writeln!(out, "//   {}. {class}{end}", i + 1);
    }
    for line in SELECTION_RULE {
        let _ = writeln!(out, "// {line}");
    }
    let _ = writeln!(
        out,
        "// Decoding ignores members it does not know. An enumeration without custom"
    );
    let _ = writeln!(
        out,
        "// values tolerates a value it does not know (LSP 3.17, \"Enumerations\"): a list"
    );
    let _ = writeln!(
        out,
        "// skips it, an optional property holding it is absent; a required one is refused."
    );
    let _ = writeln!(out);
    out.push_str(&consts);
    for texts in sections.values() {
        for text in texts {
            out.push_str(text);
            out.push('\n');
        }
    }
    let _ = writeln!(
        out,
        "// ---- decoding and encoding helpers ---------------------------------------------"
    );
    let _ = writeln!(out);
    let bases: Vec<&str> = emitter.bases.iter().copied().collect();
    for base in bases {
        out.push_str(Gen::emit_base(base));
        out.push('\n');
    }
    let helpers: Vec<String> = emitter.helpers.values().cloned().collect();
    for helper in helpers {
        out.push_str(&helper);
        out.push('\n');
    }
    while out.ends_with("\n\n") {
        out.pop();
    }
    Ok(out)
}
