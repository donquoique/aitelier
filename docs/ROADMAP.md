# AItelier roadmap

## v0.1: what the vessel pipeline needs

| # | Area | Status |
|---|---|---|
| 1 | Geometry core: points, prims, attributes, groups, transforms, bounds, normals, topology check | **done (M1)** |
| 2 | Node graph engine: DAG, parameters, cooking + caching, JSON format, expressions | **done (M1)**; node groups (subgraphs) to do |
| 3 | Basic shapes: box, grid, sphere, cylinder, circle, curve | **done (M1)**; torus, line to do |
| 4 | Modeling operators | **done**: revolve, transform, merge, extrude, sweep, boolean (own BSP implementation), mirror; to do: thicken, subdivide, smooth, delete by group |
| 5 | Instancing | **done**: copy to points, array; to do: scatter |
| 6 | Attributes and logic: set/randomize attributes, noise, switch, repeat/for-each, script node (Rhai) | expressions done; rest to do |
| 7 | UVs and materials: projections, PBR material, procedural textures, image textures | revolve/primitive UVs done; rest to do |
| 8 | Scene: camera, lights, environment map, background | to do |
| 9 | Renderer (wgpu, headless): color, depth, normals, object mask | to do |
| 10 | Data generator: parameter sampling with seeds, domain randomization, batch runs, JSON labels, split by shape | `--set` overrides done; rest to do |
| 11 | Import/export: OBJ export done; glTF export/import, graph save/load | partly done |
| 12b | Asset library: built-in examples + My library, thumbnails, open / insert / save / delete | **done** |
| 12 | Editor GUI: node editor, parameter panel with keyframes, viewport, timeline, undo | **done (M2)**; GPU viewport, curve editor, node groups UI to do |
| A | Animation: timeline, keyframes on any parameter, bezier/linear/constant/ease, time in expressions, per-frame cache, frame-sequence export | **done (M1)**; camera presets, per-frame labels with rendering to do |

## Milestones

- **M1 (done):** geometry core, graph engine, keyframes, first nodes, CLI with OBJ export.
- **M2a (done):** native editor (egui): node editor, parameters with keyframes and expressions, CPU viewport, timeline, undo/redo; `aitelier render` PNG previews.
- **M2b:** glTF export (with animation), scene nodes (camera, lights), materials, thicken/extrude/boolean.
- **M3:** wgpu renderer with color/depth/normal/mask passes, headless.
- **M4:** data generator: parameter sampling, domain randomization, labels, dataset splits.
- **M5:** vessel generator family + first dataset.

## Later (v0.2, v0.3+)

- v0.2: bevel/inset/polygon extrude (half-edge mesh), render-and-compare, model inference hook, optional Blender Cycles handoff, curve editor in the viewer.
- v0.3+: rigid bodies (Rapier), joints, cloth/soft bodies (XPBD), destruction engine as fracture solver, video rendering, simulation caching.

## Deliberately left out

Sculpting, character rigging, video editing, compositor, Grease Pencil, full node-editor GUI,
volumes/VDB, Cycles-level path tracer.
