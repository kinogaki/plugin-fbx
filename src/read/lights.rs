//! FBX's cameras and lights as the standard camera and light. A camera's
//! lens and film back are millimetres as the file holds them, its distances
//! the file's units (into the project's); an orthographic camera's view is
//! its `OrthoZoom` in file units. A light's intensity is approximated (FBX
//! holds each application's own units as a percentage), its decay left to
//! the light's falloff, said.
use super::geometry::COLORS;
use crate::fbx::{AreaShape, Camera as FbxCamera, Decay, Light as FbxLight, LightKind, Projection};
use kinogaki_render_api::standard::{Light, Shape};
use kinogaki_render_api::{Camera, Projection as Lens};
use kinogaki_studio_api::Error;
use kinogaki_studio_api::color::ColorService;

/// `camera` as the standard lens: distances times `scale` (the project's
/// units per file unit); an orthographic view `unit` centimetres a file
/// unit wide per `OrthoZoom`.
pub(super) fn camera(camera: &FbxCamera, scale: f64, unit: f64) -> Camera {
    let [width, height] = camera.film_size;
    let (projection, horizontal, vertical) = match camera.projection {
        Projection::Perspective => (Lens::Perspective, width, height),
        Projection::Orthographic => {
            let across = camera.ortho_zoom * unit * 10.0;
            (
                Lens::Orthographic,
                across,
                across * height / width.max(f64::MIN_POSITIVE),
            )
        }
    };
    let length = |v: f64| (v * scale) as f32;
    Camera {
        projection,
        focal_length: camera.focal_length as f32,
        horizontal_aperture: horizontal as f32,
        vertical_aperture: vertical as f32,
        horizontal_aperture_offset: camera.film_offset[0] as f32,
        vertical_aperture_offset: camera.film_offset[1] as f32,
        clipping_range: camera.clip.map(length),
        focus_distance: length(camera.focus_distance),
        ..Camera::default()
    }
}

/// `light` as the standard light, its sizes the file's unit times `scale`;
/// none for a volume light. Its intensity is FBX's percentage over 100.
pub(super) fn light(
    light: &FbxLight,
    scale: f64,
    colors: &dyn ColorService,
) -> Result<Option<Light>, Error> {
    let unit = (scale as f32).max(f32::MIN_POSITIVE);
    let shape = match (light.kind, light.area_shape) {
        (LightKind::Point, _) | (LightKind::Area, AreaShape::Sphere) => Shape::Spot {
            radius: unit * 0.1,
            cone_angle: 180.0,
            cone_softness: 0.0,
        },
        (LightKind::Spot, _) => {
            let [inner, outer] = light.cone.map(|a| a as f32);
            Shape::Spot {
                radius: unit * 0.1,
                cone_angle: (outer / 2.0).clamp(0.0, 180.0),
                cone_softness: if outer > 0.0 {
                    ((outer - inner) / outer).clamp(0.0, 1.0)
                } else {
                    0.0
                },
            }
        }
        (LightKind::Directional, _) => Shape::Distant { angle: 0.0 },
        (LightKind::Area, AreaShape::Rectangle) => Shape::Rect {
            width: unit,
            height: unit,
        },
        (LightKind::Volume, _) => return Ok(None),
    };
    let mut color = [light.color];
    colors.to_working(COLORS, &mut color)?;
    Ok(Some(Light {
        shape,
        color: color[0].map(|c| c as f32),
        intensity: (light.intensity / 100.0) as f32,
        normalize: matches!(light.kind, LightKind::Area),
        ..Light::default()
    }))
}

/// What reading `light` approximates, said once a file.
pub(super) fn approximated(light: &FbxLight) -> Option<&'static str> {
    match (light.kind, light.decay) {
        (LightKind::Volume, _) => Some("volume lights are left out"),
        (LightKind::Area, _) => {
            Some("area lights' sizes and every light's intensity are approximated")
        }
        (_, Decay::Quadratic) => Some("lights' intensities are approximated"),
        (_, _) => Some("lights' intensities are approximated, their decay as a physical falloff"),
    }
}
