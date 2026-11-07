//! An object's properties: its `Properties70` over the defaults its class's
//! template gives (`Definitions`).
use crate::fbx::tree::{Node, Value};
use std::collections::HashMap;
use std::sync::Arc;

/// One property: `P: "Name", "Type", "Label", "Flags", values…`.
#[derive(Clone, Debug, PartialEq)]
pub struct Property {
    /// Its name (`DiffuseColor`, `Lcl Translation`, `Maya|baseColor`…).
    pub name: String,
    /// Its type (`Color`, `double`, `Vector3D`, `KString`, `enum`…).
    pub kind: String,
    /// Its label or subtype (`Color`, `Number`, `XRefUrl`…), often empty.
    pub label: String,
    /// Its flags: `A` animatable, `U` user-defined, `+` animated.
    pub flags: String,
    /// Its values: none for a compound, three for a vector.
    pub values: Vec<Value>,
}

/// An object's properties: what it says, then its template's defaults.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Properties {
    /// What the object's `Properties70` says, in order.
    pub own: Vec<Property>,
    /// Its class's defaults, shared by every object of the class.
    pub template: Option<Arc<Template>>,
}

/// A class's default properties (`PropertyTemplate`).
#[derive(Debug, Default, PartialEq)]
pub struct Template {
    /// The defaults by name.
    pub properties: HashMap<String, Property>,
}

impl Property {
    /// `P: "Name", "Type", "Label", "Flags", values…`, or `None` when the
    /// node has too few values.
    pub(crate) fn from_node(node: &Node) -> Option<Self> {
        let mut values = node.values.iter();
        let mut text = || values.next().and_then(Value::as_str).map(str::to_owned);
        let (name, kind, label, flags) = (text()?, text()?, text()?, text()?);
        Some(Self {
            name,
            kind,
            label,
            flags,
            values: node.values[4..].to_vec(),
        })
    }

    /// Value `i` as a number.
    pub fn number(&self, i: usize) -> Option<f64> {
        self.values.get(i)?.as_float()
    }
}

impl Properties {
    /// The `Properties70` of `node`, over `template`.
    pub(crate) fn of(node: &Node, template: Option<Arc<Template>>) -> Self {
        let own = node
            .child("Properties70")
            .map(|p| {
                p.children_named("P")
                    .filter_map(Property::from_node)
                    .collect()
            })
            .unwrap_or_default();
        Self { own, template }
    }

    /// The property named `name`: the object's own, else its template's.
    pub fn get(&self, name: &str) -> Option<&Property> {
        self.own
            .iter()
            .find(|p| p.name == name)
            .or_else(|| self.template.as_ref()?.properties.get(name))
    }

    /// Whether the object itself says `name`.
    pub fn says(&self, name: &str) -> bool {
        self.own.iter().any(|p| p.name == name)
    }

    /// The first value of `name` as a number.
    pub fn number(&self, name: &str) -> Option<f64> {
        self.get(name)?.number(0)
    }

    /// The first value of `name` as an integer (enums, booleans).
    pub fn int(&self, name: &str) -> Option<i64> {
        self.get(name)?.values.first()?.as_int()
    }

    /// The first value of `name` as a string.
    pub fn string(&self, name: &str) -> Option<&str> {
        self.get(name)?.values.first()?.as_str()
    }

    /// The three values of `name` (a vector or a color).
    pub fn vec3(&self, name: &str) -> Option<[f64; 3]> {
        let p = self.get(name)?;
        Some([p.number(0)?, p.number(1)?, p.number(2)?])
    }

    /// `name`'s number, else `default`.
    pub(crate) fn number_or(&self, name: &str, default: f64) -> f64 {
        self.number(name).unwrap_or(default)
    }

    /// `name`'s vector, else `default`.
    pub(crate) fn vec3_or(&self, name: &str, default: [f64; 3]) -> [f64; 3] {
        self.vec3(name).unwrap_or(default)
    }
}

impl Template {
    /// The defaults in a `PropertyTemplate` node.
    pub(crate) fn of(node: &Node) -> Self {
        let properties = node
            .child("Properties70")
            .map(|p| {
                p.children_named("P")
                    .filter_map(Property::from_node)
                    .map(|p| (p.name.clone(), p))
                    .collect()
            })
            .unwrap_or_default();
        Self { properties }
    }
}
