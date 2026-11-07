//! What a material's network is built with: its nodes by MaterialX
//! definition, values converted from the file's, and textures as Image
//! nodes wired into texture nodes of the model's own kind (MaterialX's
//! `image`, glTF PBR's `gltf_image`, USD Preview Surface's `UsdUVTexture`),
//! one Image node per file and color in a network.
use super::geometry::uv_name;
use crate::fbx::{Property, Texture, Video, Wrap};
use kinogaki_mtlx::{NodeDef, Type, library};
use kinogaki_studio_api::Value;
use kinogaki_studio_api::color::{ColorService, LIN_REC709};
use kinogaki_studio_api::format::Unique;
use kinogaki_studio_api::format::files::Named;
use kinogaki_studio_api::geometry::image::{EmbeddedFile, ImageColor, ImageFile, ImageSource};
use kinogaki_studio_api::image_files::image_node;
use kinogaki_studio_api::network::{NetworkSeed, NewNode};
use std::path::Path;
use std::sync::Arc;

/// The files a scene's textures read: each video's, embedded where the
/// file packs it (its bytes moved out of the scene, never copied), else
/// reached through [`Named`] (`RelativeFilename` where the file gives one,
/// else `FileName`) and kept as a path, or read in with Embed Textures.
pub(super) struct Files<'n> {
    /// Each video's file, by its index in the scene.
    videos: Vec<Option<ImageFile>>,
    named: &'n Named,
}

impl<'n> Files<'n> {
    /// The files of `videos` (their embedded bytes taken), the files on disk
    /// reached through `named`.
    pub fn new(videos: &mut [Video], named: &'n Named) -> Files<'n> {
        let videos = (videos.iter_mut())
            .map(|v| {
                let content = std::mem::take(&mut v.content);
                (!content.is_empty())
                    .then(|| ImageFile::Embedded(EmbeddedFile::new(name_of(v), content)))
            })
            .collect();
        Files { videos, named }
    }

    /// The file `texture` reads; none for one out of reach, which `named`
    /// says.
    pub fn file(&mut self, texture: &Texture) -> Option<ImageFile> {
        if let Some(Some(file)) = texture.video.and_then(|v| self.videos.get(v)) {
            return Some(file.clone());
        }
        let name = match texture.relative_file_name.trim_start_matches(['/', '\\']) {
            "" => &texture.file_name,
            relative => relative,
        };
        self.named.image(name, None)
    }
}

/// A video's file name: its own, else its relative one's, else its name.
fn name_of(video: &Video) -> &str {
    fn file(path: &str) -> Option<&str> {
        path.rsplit(['/', '\\']).next().filter(|n| !n.is_empty())
    }
    file(&video.file_name)
        .or_else(|| file(&video.relative_file_name))
        .unwrap_or(&video.name)
}

/// How a model's textures are sampled.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Sampler {
    /// MaterialX's `image`, placed by `place2d`.
    Image,
    /// glTF PBR's `gltf_image` and `gltf_colorimage`.
    Gltf,
    /// USD Preview Surface's `UsdUVTexture`.
    Preview,
}

/// A material's network being built.
pub(super) struct Network<'a> {
    pub seed: NetworkSeed,
    images: Vec<(ImageSource, usize)>,
    names: Unique,
    colors: &'a dyn ColorService,
    /// What was approximated or left out, one line each.
    pub said: Vec<String>,
}

impl<'a> Network<'a> {
    pub fn new(colors: &'a dyn ColorService) -> Network<'a> {
        Network {
            seed: NetworkSeed::default(),
            images: Vec::new(),
            names: Unique::names(),
            colors,
            said: Vec::new(),
        }
    }

    /// A node of MaterialX definition `signature`, named `name` (unique).
    pub fn node(&mut self, signature: &str, name: &str) -> usize {
        let node = NewNode::materialx(signature).expect("a definition of MaterialX's library");
        let name = self.unique(name);
        self.seed.node(node.named(name))
    }

    /// Input `input` of node `at` holding `value`.
    pub fn set(&mut self, at: usize, input: &str, value: Value) {
        self.seed.nodes[at]
            .parameters
            .insert(input.to_owned(), value);
    }

    /// Input `input` of node `at` (of definition `def`) holding FBX
    /// property `p`'s value as its type takes it (a color from linear
    /// Rec.709 into the working space); false when it takes none.
    pub fn set_property(&mut self, at: usize, def: &NodeDef, input: &str, p: &Property) -> bool {
        let Some(port) = def.input(input) else {
            return false;
        };
        let numbers: Vec<f64> = (0..p.values.len()).map_while(|i| p.number(i)).collect();
        let value = match (&port.ty, &numbers[..]) {
            (Type::Float, [v, ..]) => Value::Number(*v),
            (Type::Integer, [v, ..]) => Value::Integer(v.round() as i64),
            (Type::Boolean, [v, ..]) => Value::Bool(*v != 0.0),
            (Type::Color3, [r, g, b, ..]) => {
                let mut rgb = [[*r, *g, *b]];
                if let Err(e) = self.colors.to_working(LIN_REC709, &mut rgb) {
                    self.said.push(format!("colors kept as they are: {e}"));
                }
                Value::Color(rgb[0])
            }
            // A color given as one number (a weight) is grey.
            (Type::Color3, [v]) => Value::Color([*v; 3]),
            (ty, n) if ty.width() > 1 && n.len() >= ty.width() => {
                Value::Numbers(n[..ty.width()].to_vec())
            }
            _ => return false,
        };
        self.set(at, input, value);
        true
    }

    /// The node `node`'s definition.
    pub fn def(&self, node: usize) -> Arc<NodeDef> {
        definition(&self.seed.nodes[node])
    }

    /// A texture node reading `texture`'s file sampled as `sampler` into a
    /// value of type `ty` (data, or a color read as the file says), placed
    /// by `uv_sets` (the meshes' set names, in order): the node and its
    /// output.
    pub fn texture(
        &mut self,
        texture: &Texture,
        files: &mut Files<'_>,
        sampler: Sampler,
        ty: &Type,
        uv_sets: &[String],
    ) -> (usize, &'static str) {
        let color = match ty.is_color() {
            true => ImageColor::Auto,
            false => ImageColor::Data,
        };
        let image = files.file(texture).map(|file| {
            let source = ImageSource::new(file, color);
            match self.images.iter().find(|(s, _)| *s == source) {
                Some((_, at)) => *at,
                None => {
                    let stem = Path::new(source.name())
                        .file_stem()
                        .map_or("Image".into(), |s| s.to_string_lossy());
                    let name = self.unique(&stem);
                    let at = self.seed.node(image_node::reading(&source).named(name));
                    self.images.push((source, at));
                    at
                }
            }
        });
        let wrap = |w: Wrap| -> Value {
            let mode = match w {
                Wrap::Repeat => "periodic",
                Wrap::Clamp => "clamp",
            };
            Value::Choice(Arc::from(mode))
        };
        let width = ty.width();
        let (signature, output) = match (sampler, width) {
            (Sampler::Image, 1) => ("ND_image_float", "out"),
            (Sampler::Image, 3) if ty.is_color() => ("ND_image_color3", "out"),
            (Sampler::Image, _) => ("ND_image_vector3", "out"),
            (Sampler::Gltf, 1) => ("ND_gltf_image_float_float_1_0", "out"),
            (Sampler::Gltf, _) if ty.is_color() => ("ND_gltf_colorimage", "outcolor"),
            (Sampler::Gltf, _) => ("ND_gltf_image_vector3_vector3_1_0", "out"),
            (Sampler::Preview, 1) => ("ND_UsdUVTexture_23", "r"),
            (Sampler::Preview, _) => ("ND_UsdUVTexture_23", "rgb"),
        };
        let at = self.node(signature, &texture.name);
        let (u, v) = match sampler {
            Sampler::Preview => ("wrapS", "wrapT"),
            _ => ("uaddressmode", "vaddressmode"),
        };
        self.set(at, u, wrap(texture.wrap[0]));
        self.set(at, v, wrap(texture.wrap[1]));
        if let Some(image) = image {
            self.seed.wire(image, image_node::IMAGE, at, "file");
        }
        self.coordinates(at, texture, sampler, uv_sets);
        (at, output)
    }

    /// The texture coordinates of texture node `at`: its UV set (by name
    /// among `uv_sets`, else the first, which a texture's unwired input
    /// reads; a `place2d`'s reads none, so it is wired one) and its
    /// placement.
    fn coordinates(&mut self, at: usize, texture: &Texture, sampler: Sampler, uv_sets: &[String]) {
        let set = uv_sets
            .iter()
            .position(|s| *s == texture.uv_set)
            .unwrap_or(0);
        let placed = texture.translation != [0.0; 3]
            || texture.rotation != [0.0; 3]
            || texture.scaling != [1.0; 3];
        let [tu, tv, _] = texture.translation;
        let [su, sv, _] = texture.scaling;
        let rotate = texture.rotation[2];
        let input = if sampler == Sampler::Preview {
            "st"
        } else {
            "texcoord"
        };
        // What reads the set into `to`'s `input`: nothing where the input
        // reads the first set unwired.
        let reader = |n: &mut Self, to: usize, input: &str| -> Option<usize> {
            let unwired = definition(&n.seed.nodes[to])
                .input(input)?
                .geomprop
                .is_some();
            match (sampler, set) {
                (Sampler::Preview, _) => {
                    let r = n.node("ND_UsdPrimvarReader_vector2", "UVs");
                    n.set(r, "varname", Value::Text(uv_name(set)));
                    Some(r)
                }
                (_, 0) if unwired => None,
                (_, 0) => Some(n.node("ND_texcoord_vector2", "UVs")),
                (_, _) => {
                    let r = n.node("ND_geompropvalue_vector2", "UVs");
                    n.set(r, "geomprop", Value::Text(uv_name(set)));
                    Some(r)
                }
            }
        };
        match (sampler, placed) {
            (Sampler::Gltf, true) => {
                self.set(at, "offset", Value::Numbers(vec![tu, tv]));
                self.set(at, "scale", Value::Numbers(vec![su, sv]));
                self.set(at, "rotate", Value::Number(rotate));
                if let Some(r) = reader(self, at, input) {
                    self.seed.wire(r, "out", at, input);
                }
            }
            (Sampler::Image, true) => {
                let place = self.node("ND_place2d_vector2", "Placement");
                self.set(place, "offset", Value::Numbers(vec![tu, tv]));
                self.set(place, "scale", Value::Numbers(vec![su, sv]));
                self.set(place, "rotate", Value::Number(rotate));
                if let Some(r) = reader(self, place, "texcoord") {
                    self.seed.wire(r, "out", place, "texcoord");
                }
                self.seed.wire(place, "out", at, input);
            }
            (Sampler::Preview, true) => {
                let place = self.node("ND_UsdTransform2d", "Placement");
                self.set(place, "translation", Value::Numbers(vec![tu, tv]));
                self.set(place, "scale", Value::Numbers(vec![su, sv]));
                self.set(place, "rotation", Value::Number(rotate));
                if let Some(r) = reader(self, place, "in") {
                    self.seed.wire(r, "out", place, "in");
                }
                self.seed.wire(place, "out", at, input);
            }
            (_, false) => {
                if let Some(r) = reader(self, at, input) {
                    self.seed.wire(r, "out", at, input);
                }
            }
        }
    }

    /// An Image node reading `texture`'s file, wired into nothing: a map
    /// the material keeps but doesn't use (none for a file out of reach).
    pub fn unused(&mut self, texture: &Texture, files: &mut Files<'_>) {
        let Some(file) = files.file(texture) else {
            return;
        };
        let source = ImageSource::new(file, ImageColor::Auto);
        if !self.images.iter().any(|(s, _)| *s == source) {
            let name = self.unique(&texture.name);
            let at = self.seed.node(image_node::reading(&source).named(name));
            self.images.push((source, at));
        }
    }

    /// `name`, or `name_1`, `name_2`… not taken yet.
    fn unique(&mut self, name: &str) -> String {
        self.names.take(name)
    }
}

/// The definition MaterialX node `node` holds.
pub(super) fn definition(node: &NewNode) -> Arc<NodeDef> {
    use kinogaki_render_api::material::mtlx::SIGNATURE;
    let Some(Value::Choice(signature)) = node.parameters.get(SIGNATURE) else {
        unreachable!("every node built here is MaterialX's");
    };
    library().def(signature).expect("its definition").clone()
}
