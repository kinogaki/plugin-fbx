"""Writes the Blender file the benchmark reads (README "Performance"): a
noisy grid of a million triangles with its UV map, binary FBX 7.4. Not
committed: run headless (never with a window),

    cd <somewhere> && Blender --background --factory-startup \
        --python-exit-code 1 --python <repo>/tools/blender_bench.py
"""
import math

import bpy

bpy.ops.wm.read_factory_settings(use_empty=True)
bpy.ops.mesh.primitive_grid_add(x_subdivisions=1001, y_subdivisions=501, size=10)
grid = bpy.context.object
grid.name = "Terrain"
for v in grid.data.vertices:
    v.co.z = 0.2 * math.sin(v.co.x * 4.0) * math.cos(v.co.y * 3.0)
grid.data.shade_smooth()
grid.modifiers.new("Triangulate", "TRIANGULATE")
bpy.ops.export_scene.fbx(filepath="terrain_1m_blender.fbx", bake_anim=False, use_mesh_modifiers=True)
