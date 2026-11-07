//! Cameras and lights: the `NodeAttribute`s a node shows.
//!
//! An FBX camera looks along its node's +X with +Y up, and a light shines
//! along its node's −Y; exporters turn the node (a post-rotation) to match.
use super::properties::Properties;

/// Millimetres in an inch: FBX keeps film sizes in inches.
const MM_PER_INCH: f64 = 25.4;

/// A camera.
#[derive(Clone, Debug, PartialEq)]
pub struct Camera {
    /// Its attribute's id.
    pub id: i64,
    /// Its name.
    pub name: String,
    /// How it projects (`CameraProjectionType`).
    pub projection: Projection,
    /// `FocalLength`, millimetres.
    pub focal_length: f64,
    /// The film back (`FilmWidth`, `FilmHeight`), millimetres.
    pub film_size: [f64; 2],
    /// The film's offset (`FilmOffsetX`, `FilmOffsetY`), millimetres.
    pub film_offset: [f64; 2],
    /// `NearPlane` and `FarPlane`, in file units.
    pub clip: [f64; 2],
    /// `OrthoZoom`: an orthographic camera's extent, in units the writer
    /// chose (Maya's are 30 file units).
    pub ortho_zoom: f64,
    /// `AspectWidth` and `AspectHeight`: the render's size or its ratio,
    /// as `AspectRatioMode` says.
    pub aspect: [f64; 2],
    /// `FocusDistance`, file units.
    pub focus_distance: f64,
    /// Every property, over its template's defaults.
    pub properties: Properties,
}

/// How a camera projects.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Projection {
    /// Perspective (0).
    Perspective,
    /// Orthographic (1).
    Orthographic,
}

/// A light.
#[derive(Clone, Debug, PartialEq)]
pub struct Light {
    /// Its attribute's id.
    pub id: i64,
    /// Its name.
    pub name: String,
    /// What kind (`LightType`).
    pub kind: LightKind,
    /// `Color`.
    pub color: [f64; 3],
    /// `Intensity` as written: a percentage, 100 being one; each
    /// application scales its own units into it differently.
    pub intensity: f64,
    /// How it falls off (`DecayType`).
    pub decay: Decay,
    /// A spot's full cone angles in degrees: `InnerAngle` (its full
    /// strength) and `OuterAngle` (its edge).
    pub cone: [f64; 2],
    /// An area light's shape (`AreaLightShape`).
    pub area_shape: AreaShape,
    /// `CastShadows`.
    pub cast_shadows: bool,
    /// Every property, over its template's defaults.
    pub properties: Properties,
}

/// A light's kind.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LightKind {
    /// A point (0).
    Point,
    /// Parallel light from far away (1).
    Directional,
    /// A spot (2).
    Spot,
    /// An area (3).
    Area,
    /// A volume (4).
    Volume,
}

/// How a light falls off with distance.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Decay {
    /// It doesn't (0).
    None,
    /// With distance (1).
    Linear,
    /// With its square (2).
    Quadratic,
    /// With its cube (3).
    Cubic,
}

/// An area light's shape.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AreaShape {
    /// A rectangle (0).
    Rectangle,
    /// A sphere (1).
    Sphere,
}

impl Camera {
    /// The direction a camera looks in its node.
    pub const LOOK: [f64; 3] = [1.0, 0.0, 0.0];
    /// A camera's up in its node.
    pub const UP: [f64; 3] = [0.0, 1.0, 0.0];

    pub(crate) fn new(id: i64, name: &str, properties: Properties) -> Self {
        let p = &properties;
        let inches = |name: &str, default: f64| p.number_or(name, default) * MM_PER_INCH;
        Self {
            id,
            name: name.to_owned(),
            projection: match p.int("CameraProjectionType") {
                Some(1) => Projection::Orthographic,
                _ => Projection::Perspective,
            },
            focal_length: p.number_or("FocalLength", 0.0),
            film_size: [inches("FilmWidth", 0.816), inches("FilmHeight", 0.612)],
            film_offset: [inches("FilmOffsetX", 0.0), inches("FilmOffsetY", 0.0)],
            clip: [p.number_or("NearPlane", 0.0), p.number_or("FarPlane", 0.0)],
            ortho_zoom: p.number_or("OrthoZoom", 1.0),
            aspect: [
                p.number_or("AspectWidth", 0.0),
                p.number_or("AspectHeight", 0.0),
            ],
            focus_distance: p.number_or("FocusDistance", 0.0),
            properties,
        }
    }
}

impl Light {
    /// The direction a light shines in its node.
    pub const DIRECTION: [f64; 3] = [0.0, -1.0, 0.0];

    pub(crate) fn new(id: i64, name: &str, properties: Properties) -> Self {
        let p = &properties;
        // Older files name the cone angles otherwise.
        let angle = |name: &str, old: &str| p.number(name).or_else(|| p.number(old)).unwrap_or(0.0);
        Self {
            id,
            name: name.to_owned(),
            kind: match p.int("LightType") {
                Some(1) => LightKind::Directional,
                Some(2) => LightKind::Spot,
                Some(3) => LightKind::Area,
                Some(4) => LightKind::Volume,
                _ => LightKind::Point,
            },
            color: p.vec3_or("Color", [1.0; 3]),
            intensity: p.number_or("Intensity", 100.0),
            decay: match p.int("DecayType") {
                Some(1) => Decay::Linear,
                Some(2) => Decay::Quadratic,
                Some(3) => Decay::Cubic,
                _ => Decay::None,
            },
            cone: [
                angle("InnerAngle", "HotSpot"),
                angle("OuterAngle", "Cone angle"),
            ],
            area_shape: match p.int("AreaLightShape") {
                Some(1) => AreaShape::Sphere,
                _ => AreaShape::Rectangle,
            },
            cast_shadows: p.int("CastShadows").is_none_or(|v| v != 0),
            properties,
        }
    }
}
