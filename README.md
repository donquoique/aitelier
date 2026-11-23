# AItelier

A lean procedural modeler written in Rust, in the spirit of Houdini and Blender Geometry Nodes.
It exists to generate labeled 3D training data for the image-to-procedural-3D project, and
later to feed the destruction engine.

Two programs come out of the build:

- **`aitelier-editor.exe`**: the editor. A node editor, parameter panel with keyframes,
  3D viewport and timeline in one window.
- **`aitelier.exe`**: the command-line tool, for batch work (exporting, rendering previews,
  and later generating datasets).

Both read and write the same JSON graph files.

## Setup on Windows 10

1. **Install the C++ build tools** (Rust needs Microsoft's linker):
   download *Build Tools for Visual Studio* from visualstudio.microsoft.com/downloads,
   and in the installer tick **"Desktop development with C++"**.
2. **Install Rust:** download and run `rustup-init.exe` from rustup.rs and accept the defaults.
   Close and reopen your terminal afterwards.
3. **Check:** in PowerShell, `cargo --version` should print a version.
4. **Build** (in the `aitelier` folder):
   ```powershell
   cargo build --release
   cargo test
   ```
   The first build downloads libraries and takes a few minutes (longer now that the
   editor is included). The programs are then `target\release\aitelier-editor.exe`
   and `target\release\aitelier.exe`.

## Updating

Double-click **Update AItelier** (Desktop or Start menu). It downloads the latest version
from github.com/donquoique/aitelier into `C:\dev\aitelier`, builds it, refreshes the
shortcuts and starts the editor. Your own graph files elsewhere are not touched.

## Desktop shortcut

After building, double-click **`create-shortcuts.bat`** in the `aitelier` folder (in Explorer).
It puts an **AItelier** icon on the Desktop and in the Start menu, so you can start the
editor without PowerShell. To open a graph directly, drag a `.json` file onto the icon.
Right-click the Start-menu entry → *Pin to taskbar* if you like. The shortcut points at
`target\release\aitelier-editor.exe`, so after a rebuild it starts the new version automatically.

## The editor

```powershell
.\target\release\aitelier-editor.exe examples\vase.json
```

(Or start it without a file and use *File → Open*.) Always use the **release** build:
the debug build's viewport is much slower.

| Area | How to use it |
|---|---|
| **Node editor** (left) | Right-click empty space → add a node. Drag from a node's **bottom** port to another node's **top** port to connect. Drag a connected top port to disconnect. Click to select, double-click to make a node the **output** (blue flag). Drag empty space or middle-drag to pan, scroll to zoom, **H** to frame all. **Delete** / **X** deletes the selected node. Right-click a node for more. |
| **Viewport** (centre) | Left-drag orbit, right/middle-drag or Shift+drag pan, scroll zoom, **F** frame the model. Shows the output node, or the selected node when *Show in viewport* is ticked. A red message means a node has an error. |
| **Parameters** (right) | The selected node's parameters, then the **graph parameters**. Drag numbers or click to type. **◇/◆** sets or removes a keyframe at the current frame. **ƒ** switches between a number and an expression. Right-click any number for more options. Yellow = key on this frame, green = animated, blue text = expression. |
| **Timeline** (bottom) | **Space** play/pause, **←/→** step, **Home** start. Click or drag the track to scrub. Diamonds show keyframes of the graph parameters and the selected node. |

| **Library** (tab next to Parameters) | Thumbnails of the built-in examples and of **My library** (your own models, kept in `Documents\AItelier\Library`, untouched by updates). Select a model, then **Open** it, or **Insert into scene**: its nodes are added with a `…_place` transform beside your model and merged into the output. Type a name and **Save** to put the current graph in My library. |

**Ctrl+S** save, **Ctrl+O** open, **Ctrl+Z / Ctrl+Y** undo/redo. *File* also has
*Export OBJ* (current frame) and *Render PNG* (current view, 1920×1080).

## Command line

```powershell
# What node types exist, and their parameters
target\release\aitelier.exe nodes

# Inspect a graph: parameters, nodes, which ones are animated, output statistics
target\release\aitelier.exe info examples\vase.json

# Export one frame
target\release\aitelier.exe cook examples\vase.json -o out\vase.obj

# Export the whole animation (48 frames): #### becomes the frame number
target\release\aitelier.exe cook examples\vase.json --all -o out\vase_####.obj

# Change generator parameters from the command line (this is how datasets get sampled)
target\release\aitelier.exe cook examples\vase.json -o out\tall.obj --set height=30 --set belly_r=12

# Render preview images (PNG); --all renders every frame
target\release\aitelier.exe render examples\vase.json -o out\vase.png --yaw 60 --pitch 15
target\release\aitelier.exe render examples\vase.json --all -o out\frames\vase_####.png
```

Open the `.obj` in Blender with *File → Import → Wavefront (.obj)*. AItelier is Y-up like
the OBJ convention; Blender's importer converts this automatically with its default
settings (**Forward Axis = -Z, Up Axis = Y**). If a model ever lies on its side, check those two settings.

## Graph files

A graph is a JSON file with four parts:

```json
{
  "timeline":   { "start": 1, "end": 48, "fps": 24 },
  "parameters": { "height": 24, "belly_r": { "keys": [[1, 7], [24, 10], [48, 7]] } },
  "nodes": {
    "profile": { "type": "curve",   "params": { "points": [[0,0,0], ["belly_r", "height*0.35", 0]] } },
    "vase":    { "type": "revolve", "inputs": ["profile"], "params": { "divisions": 64 } }
  },
  "output": "vase"
}
```

- **parameters**: the generator's exposed controls. Later these become the AI's labels.
- **nodes**: each has a `type`, optional `inputs` (other node names) and `params`.
- Any number in `params` or `parameters` can be:
  - a number: `2.5`
  - an expression: `"height * 0.35"`, `"sin(frame * 0.1) * 2"`, `{"expr": "..."}`.
    Expressions can use `frame`, `time` (seconds), `fps` and every graph parameter.
  - keyframes: `{"keys": [[frame, value], [frame, value, "linear"], ...]}`.
    Interpolation per key: `bezier` (default, smooth, no overshoot), `linear`, `constant`, `ease`.
- Vectors are arrays, and each component can be any of the above: `[0, "frame * 15", 0]`.

Nodes that do not depend on time are cooked once and cached; animated nodes are cached per frame.

## Built-in nodes (milestone 1)

| Node | Inputs | What it does |
|---|---|---|
| `box` | 0 | Box with per-face UVs |
| `grid` | 0 | Ground plane on XZ |
| `sphere` | 0 | UV sphere, watertight |
| `cylinder` | 0 | Cylinder, watertight when capped |
| `circle` | 0 | Filled circle or closed outline |
| `rectangle` | 0 | Rectangle with optional rounded corners |
| `curve` | 0 | Polyline through points, optional smoothing with sharp corners |
| `revolve` | 1 | Lathe a profile around an axis; profiles that start and end on the axis give watertight solids |
| `extrude` | 1 | Closed shapes into solids (with taper), open curves into walls |
| `sweep` | 2 | Profile (input 1, drawn in XY) along a path (input 2): pipes, handles, rails |
| `boolean` | 2 | Union / subtract / intersect two solids |
| `copy_to_points` | 2 | A copy of input 1 on every point of input 2 |
| `array` | 1 | Copies in a row or around an axis |
| `mirror` | 1 | Mirror across a plane, keeping the original |
| `transform` | 1 | Translate / rotate / scale around a pivot |
| `merge` | 1+ | Combine inputs |
| `normal` | 1 | Recompute normals with a hard-edge angle |

Run `aitelier nodes` for every parameter, or `aitelier nodes --json` for a machine-readable list.

## Layout

```
crates/aitelier-geo     geometry core: points, prims, attributes, groups, normals, topology checks, OBJ export
crates/aitelier-anim    timeline, keyframes, interpolation
crates/aitelier-graph   graph file format, cooking engine, expressions, built-in nodes
crates/aitelier-render  CPU preview renderer (viewport and `aitelier render`)
crates/aitelier-cli     the `aitelier` command
crates/aitelier-gui     the `aitelier-editor` program (icon in assets/)
create-shortcuts.bat   Desktop / Start-menu shortcuts on Windows
update-aitelier.bat    download + build + install the latest version
examples/              vase (animated), mug, mug_solid (sweep + boolean), table, chair, bracket (holes), primitives
docs/ROADMAP.md        what comes next
```

## License

AItelier's own code is MIT OR Apache-2.0. All dependencies use permissive licenses
(no GPL code), so products built on it stay yours.
