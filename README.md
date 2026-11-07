# FBX

FBX as a read-only format plugin for [Kinogaki Studio](https://github.com/kinogaki). This repository holds the reader: binary FBX 7.1–7.7 and ASCII FBX 7.x into a plain scene of nodes, meshes with every layer, cameras, lights, materials with every property, textures and embedded media, with the file's axes and units. It knows no SDK. The plugin's boundary, which converts the scene into the public [studio-sdk](https://github.com/kinogaki/studio-sdk)'s standard values, comes with the SDK's next format contract: package `kinogaki.fbx`, format `kinogaki.format.fbx` (`.fbx`), the dylib `formats/fbx.dylib` in `plugin/`.

The library crate is `kinogaki-fbx`; the reader is its `fbx` module.

```rust
use kinogaki_fbx::fbx::{self, Axes};

let scene = fbx::read_file("model.fbx")?;
let world = scene.world_matrices();                        // each node, in the file's system
let into = scene.settings.conversion(&Axes::Y_UP, 100.0);  // that system into Y up, metres
```

## What it reads

**Files.** Binary FBX 7100–7700 (64-bit records from 7.5) and ASCII FBX 7.x. FBX 6 and older are refused with a message saying to export FBX 2011 (7.1) or later; a version from 8000 on is refused as newer than the reader knows. Arrays are raw or zlib-compressed (inflated with lib-image's DEFLATE), all decoded side by side once the tree is read. ASCII reads numbers in any spelling exporters write (`1.#INF` too), `&quot;`, bare words, and `Content` in base64.

**The scene** (`fbx::Scene`), values as the file holds them:

| | Read |
|---|---|
| Settings | Up, front and right axes with signs, and handedness; the unit (`UnitScaleFactor`, centimetres per unit) and the original one; `conversion(axes, unit)`, the matrix into another system (a mirroring between handednesses); every `GlobalSettings` property |
| Nodes | The tree (parents before children, cycles broken and said); the whole transform stack (translation, rotation in its order, scaling, pre- and post-rotation, rotation and scaling offsets and pivots); `InheritType` (RrSs, RSrs, Rrs) and `world_matrices()` composing each as it says; the geometric transform; visibility; material slots; every property, user properties too |
| Meshes | Control points; polygons (any size) and edges; normals, tangents, binormals, UV sets and color sets by name, the material of each polygon, smoothing by polygon or edge, edge and vertex creases: each with its mapping (point, corner, polygon, edge, all) and its values directly or through indices. A mesh may be shown by several nodes, each with its own materials |
| Materials | Every property over its class's template defaults, by FBX's names; the textures on each property; the family of shader that wrote it: Lambert, Phong, Standard Surface (Arnold, OSL), 3ds Max Physical, Stingray PBS, 3ds Max PBR metal/rough and spec/gloss, 3ds Max glTF and OpenPBR materials |
| Textures | `FileName` and `RelativeFilename`; the UV set; translation, rotation, scaling; wrap; every property; their media |
| Media | `Video` with `Filename`, `RelativeFilename` and the embedded bytes; a file embedded once serves every texture naming it |
| Cameras | Perspective or orthographic, focal length, film back and offset in millimetres, clip planes, ortho zoom, aspect, focus distance, every property (an FBX camera looks along its node's +X) |
| Lights | Point, directional, spot, area and volume; color; intensity as written; decay; cone angles; area shape; shadows; every property (an FBX light shines along its node's −Y) |

**Left out, each said once in `Scene::warnings`:** animation (a file is read at one time), skins (meshes in their rest pose), blend shapes, NURBS, lines and other geometry, layered textures' blending (their layers are read), normal and tangent layers after the first, and the layer elements no mapping needs (holes, visibility, polygon groups, user data). A layer whose length or indices don't fit its mesh is left out and said; a corner naming no point fails the read.

**Untrusted input.** Every array is sized from its header and checked against `fbx::Limits` before anything is allocated: a compressed array claiming more than DEFLATE can expand to fails at once. Damaged and truncated files are errors naming where reading stopped, never panics; reading stops when its `Options::cancelled` says so.

## Code

| File | Role |
|---|---|
| `src/fbx/mod.rs` | `read`, `read_with`, `read_file`: the tree, then the scene |
| `src/fbx/tree/` | The node tree both formats read into: `binary.rs`, `ascii.rs`, and `arrays.rs`, which inflates or parses every array side by side |
| `src/fbx/scene/graph.rs` | Objects by id, connections, the class templates of `Definitions` |
| `src/fbx/scene/build.rs` | The scene from the graph: meshes first (their arrays move out of the tree), then nodes, materials, textures, media, cameras, lights |
| `src/fbx/scene/mesh.rs` | Polygons and layer elements |
| `src/fbx/scene/shading.rs` | Materials and their families, textures, media |
| `src/fbx/scene/attributes.rs` | Cameras and lights |
| `src/fbx/scene/transform.rs` | The transform stack and inheritance |
| `src/fbx/scene/settings.rs` | Axes, units and their conversion |
| `src/fbx/parallel.rs` | The one place work runs side by side: scoped threads until the plugin joins the SDK, then the SDK's workers |
| `examples/dump.rs` | Prints what a file reads as (`--tree`, its node tree) |
| `examples/bench.rs` | Times reading a file, or a synthetic million triangles |

## Tests and data

- `tests/tree.rs`: synthetic trees written binary (7.1–7.7, raw and compressed) and ASCII read back; exporters' ASCII spellings; a long ASCII array cut into pieces; hostile array headers, deep nesting, truncation at every 97th byte and 3,400 mutated files fail cleanly; cancellation.
- `tests/scene.rs`: the committed exports read as their files say. Transforms are checked against Houdini's own world matrices; every FBX 7 version reads the same scene; binary and ASCII agree to ASCII's 15 digits; a Z-up export converted into Y up lands on the Y-up export; embedded media keeps the file's bytes.
- `tests/graph.rs`: synthetic scenes (each written binary and ASCII, which must read alike): template defaults, every material family, instancing, the three inheritances and the geometric transform, texture links and shared media, cycles, damaged layers, what one time leaves out.

The committed files are our own:

| Files | Written by | How |
|---|---|---|
| `tests/fixtures/houdini/` | Houdini 22.0 (its FBX exporter, FBX SDK 2020.3) | `tools/houdini_fixtures.py`, headless: one scene (a box under a rotated, non-uniformly scaled null; two UV sets, normals, point colors, two materials, a texture; a camera; four lights) as binary FBX 7.1, 7.2, 7.3, 7.4, 7.5 and 7.7, ASCII 7.4 and 7.7, FBX 6.1 (refused), Z up and Y up left-handed (converted on export), and with its texture embedded (binary and ASCII) |
| `tests/fixtures/blender/` | Blender 5.1 (its FBX exporter, binary 7.4) | `tools/blender_fixtures.py`, headless: a cube with every layer kind Blender writes, a scene of nodes, cameras and lights, a textured plane with its PNG embedded, a centimetre scene written Z up |

Both scripts write their texture, an 8 × 8 checker PNG, themselves. The files record neutral paths: Houdini's scene file and project are set to `/project`, and Blender's absolute texture path has its directory replaced by `/project____…` of the same length (`scrub` in the script; binary strings carry their lengths, so the files are otherwise as Blender wrote them). They are released with this repository under its licence.

**Local comparisons** (not part of the tests): every committed file and two exports of a million triangles read as [ufbx](https://github.com/ufbx/ufbx) 0.21.3 reads them (world matrices of every node, mesh counts, UV and color set names, diffuse colors, texture counts, embedded bytes, cameras, lights), through a small C driver built against a ufbx checkout. assimp 6.0's `assimp info` gives the same cameras, lights, materials and embedded textures.

## Performance

Best of seven reads on an M4 Max (16 cores), release build, `cargo run --release --example bench`, with a load average of 5–13 from other work:

| File | Size | Read (tree + scene) | Target |
|---|---|---|---|
| Houdini, 1M triangles, binary 7.7 (normals by corner, UVs, colors) | 46.6 MB | 104 ms (102 + 2) | 120 ms |
| Blender, 1M triangles, binary 7.4 | 24.4 MB | 28 ms | 120 ms |
| Synthetic, 1M triangles, binary 7.4 | 24.5 MB | 20 ms | 120 ms |
| Houdini, 1M triangles, ASCII 7.7 | 271.8 MB | 130 ms | 360 ms |
| Synthetic, 1M triangles, ASCII 7.4 | 120.1 MB | 83 ms | 360 ms |

A binary file's time is that of its largest array, which is one zlib stream and can't be split: Houdini's 72 MB of normals inflate in about 100 ms (lib-image's DEFLATE runs this stream at about 0.7 GB/s; the system zlib inflates the same data recompressed in about 52 ms where lib-image takes 79 ms). Everything else runs beside it, and the scene takes 2–3 ms: arrays move out of the tree rather than being copied. The benchmark files come from `tools/houdini_bench.py` and `tools/blender_bench.py` and aren't committed.

## Building

The reader builds from this repository and lib-image (its DEFLATE), checked out side by side as Studio's tree lays them out (`libs/lib-image/` two levels above this one), with the compiler the SDK pins (Rust 1.98.1):

```sh
git clone https://github.com/kinogaki/lib-image libs/lib-image
git clone https://github.com/kinogaki/plugin-fbx plugins/fbx
cd plugins/fbx
cargo test
```

## License

Dual-licensed under the MIT license ([LICENSE-MIT](LICENSE-MIT)) and the Apache License 2.0 ([LICENSE-APACHE](LICENSE-APACHE)), at your option. [NOTICE](NOTICE) says what the reader learned from other readers. FBX is a format of Autodesk, Inc.; this reader is independent of Autodesk and its FBX SDK.
