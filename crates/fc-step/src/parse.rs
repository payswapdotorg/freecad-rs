//! STEP Part 21 parsing — [`ruststep`]'s generic AST (per ADR-0002:
//! ruststep is the *parser*, this crate owns the geometry semantics)
//! decoded into the imported subset's entity table.

use std::collections::HashMap;

use std::str::FromStr as _;

use ruststep::ast::{EntityInstance, Exchange, Name, Parameter};

/// One decoded entity: `#id = NAME(params);`.
#[derive(Debug, Clone)]
pub struct Entity {
    pub name: String,
    pub params: Vec<Param>,
}

/// A decoded parameter (the subset the importer uses; anything else
/// keeps its raw form for diagnostics).
#[derive(Debug, Clone)]
pub enum Param {
    Ref(u64),
    Real(f64),
    Int(i64),
    Str(String),
    Enum(String),
    List(Vec<Param>),
    Typed(String, Vec<Param>),
    /// `$` — not provided.
    NotProvided,
}

impl Param {
    pub fn as_ref_id(&self) -> Result<u64, String> {
        match self {
            Param::Ref(id) => Ok(*id),
            other => Err(format!("expected entity reference, got {other:?}")),
        }
    }
    pub fn as_f64(&self) -> Result<f64, String> {
        match self {
            Param::Real(v) => Ok(*v),
            Param::Int(v) => Ok(*v as f64),
            other => Err(format!("expected number, got {other:?}")),
        }
    }
    pub fn as_str(&self) -> Result<&str, String> {
        match self {
            Param::Str(s) => Ok(s),
            other => Err(format!("expected string, got {other:?}")),
        }
    }
    pub fn as_list(&self) -> Result<&[Param], String> {
        match self {
            Param::List(items) => Ok(items),
            other => Err(format!("expected list, got {other:?}")),
        }
    }
}

/// The entity table of a parsed exchange structure.
#[derive(Debug)]
pub struct EntitySet {
    pub entities: HashMap<u64, Entity>,
}

impl EntitySet {
    /// Parse a complete Part 21 file.
    pub fn parse(text: &str) -> Result<Self, String> {
        let exchange = Exchange::from_str(text).map_err(|e: ruststep::error::Error| {
            format!("cannot parse STEP (not a valid Part 21 exchange structure): {e}")
        })?;
        let mut entities = HashMap::new();
        for section in &exchange.data {
            for instance in &section.entities {
                match instance {
                    EntityInstance::Simple { id, record } => {
                        // A simple record's parameter is always the list
                        // of its arguments.
                        let params = match &record.parameter {
                            Parameter::List(items) => items.iter().map(decode_one).collect(),
                            other => vec![decode_one(other)],
                        };
                        if entities
                            .insert(
                                *id,
                                Entity {
                                    name: record.name.clone(),
                                    params,
                                },
                            )
                            .is_some()
                        {
                            return Err(format!("duplicate entity id #{id}"));
                        }
                    }
                    EntityInstance::Complex { id, subsuper } => {
                        // Complex instances (unit definitions etc.) are
                        // not part of the imported subset; they stay out
                        // of the table. The millimetre length unit is
                        // asserted textually by the importer (see
                        // import.rs).
                        let _ = (id, subsuper);
                    }
                }
            }
        }
        if entities.is_empty() {
            return Err("no data entities found in STEP file".to_string());
        }
        Ok(EntitySet { entities })
    }

    pub fn get(&self, id: u64) -> Result<&Entity, String> {
        self.entities
            .get(&id)
            .ok_or_else(|| format!("reference to missing entity #{id}"))
    }

    /// `#id = NAME(p0..pn)` with the expected name.
    pub fn entity(&self, id: u64, expected: &str) -> Result<&Entity, String> {
        let e = self.get(id)?;
        if e.name != expected {
            return Err(format!("entity #{id} is a {}, expected {expected}", e.name));
        }
        Ok(e)
    }

    /// All entities of a given kind (deterministic ascending-id order).
    pub fn all(&self, name: &str) -> Vec<(u64, &Entity)> {
        let mut found: Vec<(u64, &Entity)> = self
            .entities
            .iter()
            .filter(|(_, e)| e.name == name)
            .map(|(id, e)| (*id, e))
            .collect();
        found.sort_by_key(|(id, _)| *id);
        found
    }
}

fn decode_one(parameter: &Parameter) -> Param {
    match parameter {
        Parameter::Typed { keyword, parameter } => {
            let items: Vec<Param> = match parameter.as_ref() {
                Parameter::List(list) => list.iter().map(decode_one).collect(),
                p => vec![decode_one(p)],
            };
            Param::Typed(keyword.clone(), items)
        }
        Parameter::List(list) => Param::List(list.iter().map(decode_one).collect()),
        Parameter::Integer(v) => Param::Int(*v),
        Parameter::Real(v) => Param::Real(*v),
        Parameter::String(s) => Param::Str(s.to_string()),
        Parameter::Enumeration(e) => Param::Enum(e.to_string()),
        Parameter::Ref(name) => match name {
            Name::Entity(id) => Param::Ref(*id),
            Name::Value(id) => Param::Ref(*id),
            Name::ConstantEntity(s) => Param::Str(s.clone()),
            Name::ConstantValue(v) => Param::Str(v.to_string()),
        },
        Parameter::NotProvided => Param::NotProvided,
        Parameter::Omitted => Param::NotProvided,
    }
}
