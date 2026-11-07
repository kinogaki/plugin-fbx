//! The object graph: every object of `Objects` by its id, the `Connections`
//! between them, and the class templates of `Definitions`.
//!
//! A connection names a child and a parent: `OO` an object under an object
//! (a mesh's geometry under its model, a model under its parent), `OP` an
//! object under a parent's property (a texture under a material's
//! `DiffuseColor`). The scene's root has id 0 and no object.
use super::properties::{Properties, Template};
use crate::fbx::tree::{Document, Node};
use std::collections::HashMap;
use std::sync::Arc;

/// One object of `Objects`.
pub(crate) struct Object<'d> {
    pub id: i64,
    /// Its class, the node's name: `Model`, `Geometry`, `Material`…
    pub class: &'d str,
    /// Its name without the `Class::` prefix.
    pub name: &'d str,
    /// Its subclass: `Mesh`, `Null`, `Camera`, `LimbNode`…
    pub subclass: &'d str,
    pub node: &'d Node,
}

/// One end of a connection.
#[derive(Clone, Copy)]
pub(crate) struct Link<'d> {
    pub object: usize,
    /// The parent's property an `OP` connection names.
    pub property: Option<&'d str>,
}

pub(crate) struct Graph<'d> {
    pub objects: Vec<Object<'d>>,
    /// For each object, what is connected under it, in file order.
    children: Vec<Vec<Link<'d>>>,
    /// For each object, what it is connected under, in file order.
    parents: Vec<Vec<Link<'d>>>,
    /// Templates by class, with their names.
    templates: HashMap<&'d str, Vec<(&'d str, Arc<Template>)>>,
}

impl<'d> Graph<'d> {
    pub fn new(document: &'d Document) -> Self {
        let objects: Vec<Object> = document
            .node("Objects")
            .map(|o| o.children.iter().filter_map(object).collect())
            .unwrap_or_default();
        let by_id: HashMap<i64, usize> =
            objects.iter().enumerate().map(|(i, o)| (o.id, i)).collect();
        let mut children = vec![Vec::new(); objects.len()];
        let mut parents = vec![Vec::new(); objects.len()];
        let connections = document.node("Connections").map(|c| c.children_named("C"));
        for c in connections.into_iter().flatten() {
            let (kind, child, parent) = (c.str(0), c.int(1), c.int(2));
            let property = match kind {
                Some("OO") => None,
                Some("OP") => c.str(3),
                _ => continue,
            };
            let (Some(&child), Some(&parent)) = (
                child.and_then(|i| by_id.get(&i)),
                parent.and_then(|i| by_id.get(&i)),
            ) else {
                continue;
            };
            children[parent].push(Link {
                object: child,
                property,
            });
            parents[child].push(Link {
                object: parent,
                property,
            });
        }
        let mut templates: HashMap<&str, Vec<_>> = HashMap::new();
        let types = document
            .node("Definitions")
            .map(|d| d.children_named("ObjectType"));
        for kind in types.into_iter().flatten() {
            let Some(class) = kind.str(0) else { continue };
            for template in kind.children_named("PropertyTemplate") {
                let name = template.str(0).unwrap_or("");
                templates
                    .entry(class)
                    .or_default()
                    .push((name, Arc::new(Template::of(template))));
            }
        }
        Self {
            objects,
            children,
            parents,
            templates,
        }
    }

    /// Indices of the objects of `class`.
    pub fn of_class(&self, class: &'d str) -> impl Iterator<Item = usize> + '_ {
        (0..self.objects.len()).filter(move |&i| self.objects[i].class == class)
    }

    /// What is connected under `object`.
    pub fn children(&self, object: usize) -> &[Link<'d>] {
        &self.children[object]
    }

    /// What `object` is connected under.
    pub fn parents(&self, object: usize) -> &[Link<'d>] {
        &self.parents[object]
    }

    /// The objects of `class` connected under `object`, in order.
    pub fn children_of_class(
        &self,
        object: usize,
        class: &'d str,
    ) -> impl Iterator<Item = usize> + '_ {
        self.children[object]
            .iter()
            .map(|l| l.object)
            .filter(move |&c| self.objects[c].class == class)
    }

    /// `object`'s properties over its class's template.
    pub fn properties(&self, object: usize) -> Properties {
        let o = &self.objects[object];
        Properties::of(o.node, self.template(o))
    }

    /// The template for `object`: the one whose name ends with its subclass
    /// or shading model (`FbxCamera` for a camera attribute,
    /// `FbxSurfacePhong` for a phong material), else its class's first. A
    /// node attribute takes only its own kind's: a light never takes a
    /// camera's defaults.
    fn template(&self, object: &Object) -> Option<Arc<Template>> {
        let candidates = self.templates.get(object.class)?;
        let model = object.node.child_str("ShadingModel").unwrap_or("");
        let own = candidates.iter().find(|(name, _)| {
            let name = name.to_ascii_lowercase();
            [object.subclass, model]
                .iter()
                .any(|s| !s.is_empty() && name.ends_with(&s.to_ascii_lowercase()))
        });
        let fallback = || {
            candidates
                .first()
                .filter(|_| object.class != "NodeAttribute")
        };
        own.or_else(fallback).map(|(_, t)| t.clone())
    }
}

/// An object from its node: `Class: id, "Class::Name", "Subclass" {…}`.
fn object(node: &Node) -> Option<Object<'_>> {
    Some(Object {
        id: node.int(0)?,
        class: &node.name,
        name: object_name(node),
        subclass: node.str(2).unwrap_or(""),
        node,
    })
}

/// An object node's name without its `Class::` prefix.
pub(crate) fn object_name(node: &Node) -> &str {
    let full = node.str(1).unwrap_or("");
    full.strip_prefix(node.name.as_str())
        .and_then(|rest| rest.strip_prefix("::"))
        .unwrap_or(full)
}
