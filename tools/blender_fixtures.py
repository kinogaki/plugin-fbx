"""Writes the Blender test files of tests/fixtures/blender/ (README "Test data").

Run headless (never with a window), from the directory the files go to (the
files record their names, so they are written by relative path):

    cd tests/fixtures/blender && Blender --background --factory-startup \
        --python-exit-code 1 --python ../../../tools/blender_fixtures.py

Four files, binary FBX 7.4 (Blender writes no other):
- mesh.fbx: a cube of six quads with two UV maps, a corner and a point color
  attribute, two materials, sharp edges (smoothing by edge), tangents and
  custom split normals;
- scene.fbx: an empty rotated XYZ and scaled non-uniformly, a child cube
  rotated ZXY; a perspective and an orthographic camera; a point, a spot, a
  sun and an area light;
- textured.fbx: a plane whose material reads an 8 × 8 PNG for its base color
  and normal map, the PNG embedded;
- zup_cm.fbx: a scene in centimetres written Z up, Y forward.

Blender writes a texture's absolute path beside its relative one; `scrub`
replaces the directory the files were written in with a neutral one.
"""
import math
import os
import shutil
import struct
import zlib

import bpy


def clear():
    bpy.ops.wm.read_factory_settings(use_empty=True)


def png(path, width, height):
    """A small RGB PNG of a checker, written without any image library."""
    rows = b""
    for y in range(height):
        row = b"\x00"
        for x in range(width):
            on = (x // 2 + y // 2) % 2
            row += bytes((230, 120, 20) if on else (40, 40, 40))
        rows += row

    def chunk(kind, data):
        body = kind + data
        return struct.pack(">I", len(data)) + body + struct.pack(">I", zlib.crc32(body))

    header = struct.pack(">IIBBBBB", width, height, 8, 2, 0, 0, 0)
    with open(path, "wb") as f:
        f.write(b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", header) + chunk(b"IDAT", zlib.compress(rows)) + chunk(b"IEND", b""))


def principled(name, color, roughness=0.5, metallic=0.0):
    mat = bpy.data.materials.new(name)
    mat.use_nodes = True
    bsdf = mat.node_tree.nodes["Principled BSDF"]
    bsdf.inputs["Base Color"].default_value = (*color, 1)
    bsdf.inputs["Roughness"].default_value = roughness
    bsdf.inputs["Metallic"].default_value = metallic
    return mat


def cube(name):
    bpy.ops.mesh.primitive_cube_add(size=2)
    obj = bpy.context.object
    obj.name = name
    obj.data.name = name
    return obj


def export(path, **options):
    bpy.ops.export_scene.fbx(filepath=path, bake_anim=False, **options)
    scrub(path)


def scrub(path):
    """Blender records a texture by its absolute path too: the directory this
    ran in is replaced by a neutral one of the same length, byte for byte, so
    no machine's path is kept and the file's structure is unchanged (binary
    FBX strings carry their lengths)."""
    here = (os.getcwd() + "/").encode()
    neutral = (b"/project" + b"_" * len(here))[: len(here) - 1] + b"/"
    with open(path, "rb") as f:
        data = f.read()
    with open(path, "wb") as f:
        f.write(data.replace(here, neutral))


def mesh_file():
    clear()
    obj = cube("Layered")
    mesh = obj.data
    mesh.uv_layers.new(name="Second")
    for i, loop in enumerate(mesh.uv_layers["Second"].data):
        loop.uv = (i / 24.0, 1.0 - i / 24.0)
    corner = mesh.color_attributes.new("Corner", "BYTE_COLOR", "CORNER")
    for i, c in enumerate(corner.data):
        c.color = (i / 23.0, 0.25, 1.0 - i / 23.0, 1.0)
    point = mesh.color_attributes.new("Point", "FLOAT_COLOR", "POINT")
    for i, c in enumerate(point.data):
        c.color = (0.1 * i, 0.5, 0.2, 0.5)
    mesh.materials.append(principled("Red", (0.8, 0.05, 0.02), 0.3, 0.0))
    mesh.materials.append(principled("Metal", (0.9, 0.9, 0.85), 0.2, 1.0))
    for poly in mesh.polygons:
        poly.material_index = 0 if poly.index < 3 else 1
        poly.use_smooth = True
    for edge in mesh.edges[:4]:
        edge.use_edge_sharp = True
    normals = [(0.0, 0.0, 1.0)] * len(mesh.loops)
    mesh.normals_split_custom_set(normals)
    export("mesh.fbx", mesh_smooth_type="EDGE", use_tspace=True, colors_type="LINEAR")


def scene_file():
    clear()
    bpy.ops.object.empty_add()
    rig = bpy.context.object
    rig.name = "Rig"
    rig.rotation_euler = (math.radians(10), math.radians(20), math.radians(30))
    rig.scale = (2.0, 1.0, 0.5)
    child = cube("Child")
    child.parent = rig
    child.location = (1.0, 2.0, 3.0)
    child.rotation_mode = "ZXY"
    child.rotation_euler = (math.radians(15), math.radians(25), math.radians(35))

    bpy.ops.object.camera_add(location=(0, -8, 2), rotation=(math.radians(80), 0, 0))
    cam = bpy.context.object
    cam.name = "Perspective"
    cam.data.lens = 50
    cam.data.sensor_width = 36
    cam.data.sensor_fit = "HORIZONTAL"
    cam.data.shift_x = 0.1
    cam.data.clip_start = 0.1
    cam.data.clip_end = 100
    bpy.ops.object.camera_add(location=(8, 0, 2))
    ortho = bpy.context.object
    ortho.name = "Orthographic"
    ortho.data.type = "ORTHO"
    ortho.data.ortho_scale = 6

    def light(kind, name, location, **settings):
        bpy.ops.object.light_add(type=kind, location=location)
        obj = bpy.context.object
        obj.name = name
        for key, value in settings.items():
            setattr(obj.data, key, value)
        return obj

    light("POINT", "Point", (3, 3, 3), energy=1000, color=(1.0, 0.9, 0.8))
    light("SPOT", "Spot", (-3, 3, 3), energy=500, spot_size=math.radians(45), spot_blend=0.15)
    light("SUN", "Sun", (0, 0, 10), energy=3)
    light("AREA", "Area", (0, 3, 5), energy=200, shape="RECTANGLE", size=2, size_y=1)
    export("scene.fbx")


def textured_file():
    clear()
    png("checker.png", 8, 8)
    bpy.ops.mesh.primitive_plane_add(size=2)
    obj = bpy.context.object
    obj.name = "Plane"
    mat = principled("Checker", (1, 1, 1))
    nodes, links = mat.node_tree.nodes, mat.node_tree.links
    image = bpy.data.images.load(os.path.abspath("checker.png"))
    base = nodes.new("ShaderNodeTexImage")
    base.image = image
    links.new(base.outputs["Color"], nodes["Principled BSDF"].inputs["Base Color"])
    normal_image = nodes.new("ShaderNodeTexImage")
    normal_image.image = image
    normal_map = nodes.new("ShaderNodeNormalMap")
    normal_map.inputs["Strength"].default_value = 0.5
    links.new(normal_image.outputs["Color"], normal_map.inputs["Color"])
    links.new(normal_map.outputs["Normal"], nodes["Principled BSDF"].inputs["Normal"])
    obj.data.materials.append(mat)
    export("textured.fbx", path_mode="COPY", embed_textures=True)
    os.remove("checker.png")
    shutil.rmtree("textured.fbm", ignore_errors=True)


def zup_cm_file():
    clear()
    bpy.context.scene.unit_settings.system = "METRIC"
    bpy.context.scene.unit_settings.scale_length = 0.01
    obj = cube("Centimetres")
    obj.location = (10.0, 20.0, 30.0)
    export("zup_cm.fbx", axis_up="Z", axis_forward="Y", apply_unit_scale=True)


mesh_file()
scene_file()
textured_file()
zup_cm_file()
