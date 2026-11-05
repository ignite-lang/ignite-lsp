//! The parts of the LSP meta model (`metaModel.json`, schema `metaModel.schema.json` of
//! vscode-languageserver-node) the generator reads: requests, notifications, structures,
//! enumerations and type aliases. A type of a kind this reader does not know is kept as
//! [`Type::Unknown`], so the generator refuses it only when a root reaches it.

use crate::json::Json;

#[derive(Clone, Debug, PartialEq)]
pub enum Type {
    Base(String),
    Reference(String),
    Array(Box<Type>),
    Map(Box<Type>, Box<Type>),
    Or(Vec<Type>),
    Literal(Vec<Property>),
    StringLiteral(String),
    /// `and`, `tuple`, `integerLiteral`, `booleanLiteral` or any other kind: the kind's name.
    Unknown(String),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Property {
    pub name: String,
    pub ty: Type,
    pub optional: bool,
}

#[derive(Clone, Debug)]
pub struct Structure {
    pub name: String,
    pub extends: Vec<Type>,
    pub mixins: Vec<Type>,
    pub properties: Vec<Property>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Text(String),
    Int(i64),
}

#[derive(Clone, Debug)]
pub struct Enumeration {
    pub name: String,
    /// `string`, `integer` or `uinteger`.
    pub base: String,
    pub values: Vec<(String, Value)>,
    pub custom: bool,
}

#[derive(Clone, Debug)]
pub struct Alias {
    pub name: String,
    pub ty: Type,
}

#[derive(Clone, Debug)]
pub struct Message {
    pub method: String,
    pub request: bool,
    pub params: Option<Type>,
    pub result: Option<Type>,
}

#[derive(Clone, Debug)]
pub struct MetaModel {
    pub version: String,
    pub messages: Vec<Message>,
    pub structures: Vec<Structure>,
    pub enumerations: Vec<Enumeration>,
    pub aliases: Vec<Alias>,
}

fn text<'a>(value: &'a Json, key: &str, what: &str) -> Result<&'a str, String> {
    value
        .get(key)
        .and_then(Json::as_str)
        .ok_or_else(|| format!("{what}: `{key}` is not a string"))
}

fn list<'a>(value: &'a Json, key: &str, what: &str) -> Result<&'a [Json], String> {
    match value.get(key) {
        None => Ok(&[]),
        Some(items) => items
            .as_array()
            .ok_or_else(|| format!("{what}: `{key}` is not an array")),
    }
}

fn ty(value: &Json, what: &str) -> Result<Type, String> {
    let kind = text(value, "kind", what)?;
    Ok(match kind {
        "base" => Type::Base(text(value, "name", what)?.to_string()),
        "reference" => Type::Reference(text(value, "name", what)?.to_string()),
        "array" => Type::Array(Box::new(ty(
            value
                .get("element")
                .ok_or_else(|| format!("{what}: array without `element`"))?,
            what,
        )?)),
        "map" => {
            let key = value
                .get("key")
                .ok_or_else(|| format!("{what}: map without `key`"))?;
            let item = value
                .get("value")
                .ok_or_else(|| format!("{what}: map without `value`"))?;
            Type::Map(Box::new(ty(key, what)?), Box::new(ty(item, what)?))
        }
        "or" => Type::Or(
            list(value, "items", what)?
                .iter()
                .map(|item| ty(item, what))
                .collect::<Result<_, _>>()?,
        ),
        "literal" => {
            let body = value
                .get("value")
                .ok_or_else(|| format!("{what}: literal without `value`"))?;
            Type::Literal(properties(body, what)?)
        }
        "stringLiteral" => Type::StringLiteral(text(value, "value", what)?.to_string()),
        other => Type::Unknown(other.to_string()),
    })
}

fn properties(value: &Json, what: &str) -> Result<Vec<Property>, String> {
    list(value, "properties", what)?
        .iter()
        .map(|property| {
            let name = text(property, "name", what)?.to_string();
            let at = format!("{what}.{name}");
            let ty = ty(
                property
                    .get("type")
                    .ok_or_else(|| format!("{at}: property without `type`"))?,
                &at,
            )?;
            let optional = property
                .get("optional")
                .and_then(Json::as_bool)
                .unwrap_or(false);
            Ok(Property { name, ty, optional })
        })
        .collect()
}

fn optional_ty(value: &Json, key: &str, what: &str) -> Result<Option<Type>, String> {
    match value.get(key) {
        None | Some(Json::Null) => Ok(None),
        Some(Json::Array(_)) => Ok(Some(Type::Unknown(format!("{key} array")))),
        Some(item) => ty(item, what).map(Some),
    }
}

impl MetaModel {
    pub fn parse(source: &str) -> Result<MetaModel, String> {
        let root = Json::parse(source)?;
        let version = root
            .get("metaData")
            .map(|meta| text(meta, "version", "metaData"))
            .transpose()?
            .unwrap_or("unknown")
            .to_string();
        let mut messages = Vec::new();
        for (key, request) in [("requests", true), ("notifications", false)] {
            for message in list(&root, key, key)? {
                let method = text(message, "method", key)?.to_string();
                let what = format!("{key}/{method}");
                messages.push(Message {
                    params: optional_ty(message, "params", &what)?,
                    result: optional_ty(message, "result", &what)?,
                    method,
                    request,
                });
            }
        }
        let mut structures = Vec::new();
        for structure in list(&root, "structures", "structures")? {
            let name = text(structure, "name", "structures")?.to_string();
            let what = name.clone();
            let types = |key: &str| -> Result<Vec<Type>, String> {
                list(structure, key, &what)?
                    .iter()
                    .map(|item| ty(item, &what))
                    .collect()
            };
            structures.push(Structure {
                extends: types("extends")?,
                mixins: types("mixins")?,
                properties: properties(structure, &what)?,
                name,
            });
        }
        let mut enumerations = Vec::new();
        for enumeration in list(&root, "enumerations", "enumerations")? {
            let name = text(enumeration, "name", "enumerations")?.to_string();
            let base = match enumeration.get("type").map(|t| ty(t, &name)).transpose()? {
                Some(Type::Base(base)) => base,
                _ => return Err(format!("{name}: enumeration type is not a base type")),
            };
            let mut values = Vec::new();
            for entry in list(enumeration, "values", &name)? {
                let value_name = text(entry, "name", &name)?.to_string();
                let value = match entry.get("value") {
                    Some(Json::Str(text)) => Value::Text(text.clone()),
                    Some(Json::Number(text)) => Value::Int(text.parse().map_err(|_| {
                        format!("{name}.{value_name}: value `{text}` is not an integer")
                    })?),
                    _ => {
                        return Err(format!(
                            "{name}.{value_name}: value is not a string or number"
                        ));
                    }
                };
                values.push((value_name, value));
            }
            let custom = enumeration
                .get("supportsCustomValues")
                .and_then(Json::as_bool)
                .unwrap_or(false);
            enumerations.push(Enumeration {
                name,
                base,
                values,
                custom,
            });
        }
        let mut aliases = Vec::new();
        for alias in list(&root, "typeAliases", "typeAliases")? {
            let name = text(alias, "name", "typeAliases")?.to_string();
            let ty = ty(
                alias
                    .get("type")
                    .ok_or_else(|| format!("{name}: alias without `type`"))?,
                &name,
            )?;
            aliases.push(Alias { name, ty });
        }
        Ok(MetaModel {
            version,
            messages,
            structures,
            enumerations,
            aliases,
        })
    }
}
