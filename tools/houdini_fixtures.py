"""Writes the Houdini test files of tests/fixtures/houdini/ (README "Test data").

Run headless with Houdini's Python, from the directory the files go to (the
files record their names, so they are written by relative path):

    cd tests/fixtures/houdini && hython ../../../tools/houdini_fixtures.py

One scene, a box under a rotated and non-uniformly scaled null, with two
UV sets, normals, point colors and a material a face; a camera; a point, a
spot, a distant and an area light. It is written by every FBX version the
exporter offers, binary and ASCII, in three axis systems, and once with its
texture embedded (binary and ASCII). The other axis systems are
converted into, so their data is in the axes they name.
"""
import os
import struct
import zlib

import hou


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


def material(name, color, texture=None):
    shader = hou.node("/mat").createNode("principledshader::2.0", name)
    shader.parmTuple("basecolor").set(color)
    shader.parm("rough").set(0.4)
    if texture:
        shader.parm("basecolor_useTexture").set(1)
        shader.parm("basecolor_texture").set(texture)
    return shader


def scene():
    png("checker.png", 8, 8)
    red = material("red", (0.8, 0.1, 0.05), "checker.png")
    blue = material("blue", (0.1, 0.2, 0.9))

    obj = hou.node("/obj")
    rig = obj.createNode("null", "rig")
    rig.parmTuple("r").set((0, 30, 0))
    rig.parmTuple("s").set((2, 1, 1))

    geo = obj.createNode("geo", "box")
    geo.setFirstInput(rig)
    geo.parmTuple("t").set((0, 1, 0))
    geo.parmTuple("r").set((10, 20, 30))
    geo.parm("rOrd").set("zxy")
    geo.parmTuple("p").set((0.5, 0, 0))
    box = geo.createNode("box")
    uvs = geo.createNode("attribwrangle", "uvs")
    uvs.setFirstInput(box)
    uvs.parm("class").set("vertex")
    uvs.parm("snippet").set("v@uv = set(@P.x + 0.5, @P.y + 0.5, 0);\nv@uv2 = set(@P.x + 0.5, @P.z + 0.5, 0);")
    color = geo.createNode("attribwrangle", "color")
    color.setFirstInput(uvs)
    color.parm("class").set("point")
    color.parm("snippet").set("v@Cd = set(@ptnum / 7.0, 0.5, 1 - @ptnum / 7.0);")
    mats = geo.createNode("attribwrangle", "materials")
    mats.setFirstInput(color)
    mats.parm("class").set("primitive")
    mats.parm("snippet").set('s@shop_materialpath = @primnum < 3 ? "/mat/red" : "/mat/blue";')
    normal = geo.createNode("normal")
    normal.setFirstInput(mats)
    normal.parm("type").set("typevertex")
    normal.setDisplayFlag(True)
    normal.setRenderFlag(True)

    cam = obj.createNode("cam", "camera")
    cam.parmTuple("t").set((0, 2, 8))
    cam.parm("focal").set(35)
    cam.parm("aperture").set(36)
    cam.parm("resx").set(1920)
    cam.parm("resy").set(1080)

    point = obj.createNode("hlight::2.0", "point_light")
    point.parm("light_type").set("point")
    point.parm("light_intensity").set(2.5)
    point.parmTuple("light_color").set((1, 0.9, 0.8))
    point.parmTuple("t").set((3, 4, 2))
    spot = obj.createNode("hlight::2.0", "spot_light")
    spot.parm("light_type").set("point")
    spot.parm("coneenable").set(1)
    spot.parm("coneangle").set(40)
    spot.parmTuple("t").set((-3, 4, 2))
    sun = obj.createNode("hlight::2.0", "distant_light")
    sun.parm("light_type").set("distant")
    sun.parmTuple("r").set((-45, 30, 0))
    area = obj.createNode("hlight::2.0", "area_light")
    area.parm("light_type").set("grid")
    area.parmTuple("areasize").set((2, 1))
    area.parmTuple("t").set((0, 5, 0))


def export(name, version, ascii=False, axes="yupright", embed=False):
    rop = hou.node("/out").createNode("filmboxfbx")
    rop.parm("sopoutput").set(name)
    rop.parm("sdkversion").set(version)
    # The toggle is "Export in ASCII Format".
    rop.parm("exportkind").set(1 if ascii else 0)
    rop.parm("axissystem").set(axes)
    # Without it the exporter only labels the file with the axes.
    rop.parm("convertaxis").set(1)
    rop.parm("embedmedia").set(1 if embed else 0)
    rop.parm("trange").set("off")
    rop.render()
    rop.destroy()


# The files record the scene's file and project directory: neutral names
# keep the machine's own paths out of them.
hou.hipFile.setName("/project/fixtures.hip")
hou.putenv("HIP", "/project")
hou.putenv("JOB", "/project")
scene()
for version, year in [
    ("7100", "FBX201100"),
    ("7200", "FBX201200"),
    ("7300", "FBX201300"),
    ("7400", "FBX201400"),
    ("7500", "FBX201800"),
    ("7700", "FBX202000"),
]:
    export(f"box_{version}.fbx", f"FBX | {year}")
export("box_7400_ascii.fbx", "FBX | FBX201400", ascii=True)
export("box_7700_ascii.fbx", "FBX | FBX202000", ascii=True)
export("box_6100.fbx", "FBX 6.0 | FBX201000")
export("box_zup.fbx", "FBX | FBX202000", axes="zupright")
export("box_yup_left.fbx", "FBX | FBX202000", axes="yupleft")
export("box_embedded.fbx", "FBX | FBX202000", embed=True)
export("box_embedded_ascii.fbx", "FBX | FBX202000", ascii=True, embed=True)
os.remove("checker.png")
