"""Writes the Houdini files the benchmark reads (README "Performance"): a
noisy grid of a million triangles with vertex normals, UVs and point
colors, binary and ASCII FBX 7.7. Not committed: run headless,

    cd <somewhere> && hython <repo>/tools/houdini_bench.py
"""
import hou

hou.hipFile.setName("/project/bench.hip")
hou.putenv("HIP", "/project")
hou.putenv("JOB", "/project")
geo = hou.node("/obj").createNode("geo", "terrain")
grid = geo.createNode("grid")
grid.parmTuple("size").set((10, 5))
grid.parm("rows").set(501)
grid.parm("cols").set(1001)
shape = geo.createNode("attribwrangle", "shape")
shape.setFirstInput(grid)
shape.parm("snippet").set(
    "@P.y = noise(@P * 3) * 0.5;\n"
    "v@Cd = set(noise(@P * 7), 0.5, 0.25);\n"
)
uv = geo.createNode("attribwrangle", "uv")
uv.setFirstInput(shape)
uv.parm("class").set("vertex")
uv.parm("snippet").set("v@uv = set(@P.x / 10 + 0.5, @P.z / 5 + 0.5, 0);")
split = geo.createNode("divide")
split.setFirstInput(uv)
normal = geo.createNode("normal")
normal.setFirstInput(split)
normal.parm("type").set("typevertex")
normal.setDisplayFlag(True)
normal.setRenderFlag(True)
for name, ascii in [("terrain_1m.fbx", 0), ("terrain_1m_ascii.fbx", 1)]:
    rop = hou.node("/out").createNode("filmboxfbx")
    rop.parm("sopoutput").set(name)
    rop.parm("sdkversion").set("FBX | FBX202000")
    rop.parm("exportkind").set(ascii)
    rop.parm("trange").set("off")
    rop.render()
