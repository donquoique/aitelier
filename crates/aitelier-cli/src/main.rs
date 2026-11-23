//! `aitelier` command-line tool.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::Arc;

use aitelier_geo::{obj, Geometry};
use aitelier_graph::{Cooker, Graph, Registry, Value};
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(
    name = "aitelier",
    version,
    about = "AItelier: procedural modeling from node graphs"
)]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// List all node types and their parameters.
    Nodes {
        /// Print as JSON (for scripts and AI tools).
        #[arg(long)]
        json: bool,
    },
    /// Validate a graph and show its nodes and the output's statistics.
    Info {
        graph: PathBuf,
        /// Override a graph parameter, e.g. --set height=25 (repeatable).
        #[arg(long = "set", value_name = "NAME=VALUE")]
        set: Vec<String>,
    },
    /// Cook a graph and export the result (.obj).
    Cook {
        graph: PathBuf,
        /// Output file. For several frames use #### (frame number) in the name, e.g. out/vase_####.obj
        #[arg(short, long)]
        out: PathBuf,
        /// Frame to cook (default: timeline start).
        #[arg(short, long, conflicts_with_all = ["range", "all"])]
        frame: Option<f64>,
        /// Cook every frame from START to END.
        #[arg(long, num_args = 2, value_names = ["START", "END"], conflicts_with = "all")]
        range: Option<Vec<f64>>,
        /// Cook every frame of the graph's timeline.
        #[arg(long)]
        all: bool,
        /// Cook this node instead of the graph's output.
        #[arg(long)]
        node: Option<String>,
        /// Override a graph parameter, e.g. --set height=25 (repeatable).
        #[arg(long = "set", value_name = "NAME=VALUE")]
        set: Vec<String>,
    },
    /// Render a preview image (.png) of a graph's output. Use #### for several frames.
    Render {
        graph: PathBuf,
        #[arg(short, long)]
        out: PathBuf,
        /// Frame to render (default: timeline start).
        #[arg(short, long, conflicts_with = "all")]
        frame: Option<f64>,
        /// Render every frame of the timeline.
        #[arg(long)]
        all: bool,
        #[arg(long, default_value_t = 960)]
        width: u32,
        #[arg(long, default_value_t = 640)]
        height: u32,
        /// Camera rotation around the object, degrees.
        #[arg(long, default_value_t = 35.0)]
        yaw: f32,
        /// Camera elevation, degrees.
        #[arg(long, default_value_t = 20.0)]
        pitch: f32,
        /// Draw polygon edges.
        #[arg(long)]
        wireframe: bool,
        /// Hide the ground grid.
        #[arg(long)]
        no_grid: bool,
        /// Override a graph parameter, e.g. --set height=25 (repeatable).
        #[arg(long = "set", value_name = "NAME=VALUE")]
        set: Vec<String>,
    },
}

fn main() -> ExitCode {
    match run(Cli::parse()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

type AnyResult<T> = Result<T, Box<dyn std::error::Error>>;

fn run(cli: Cli) -> AnyResult<()> {
    let registry = Arc::new(Registry::with_builtins());
    match cli.cmd {
        Cmd::Nodes { json } => print_nodes(&registry, json),
        Cmd::Info { graph, set } => {
            let g = load(&graph, &set)?;
            let mut cooker = Cooker::new(Arc::new(g.clone()), registry.clone())?;
            let tl = g.timeline;
            println!(
                "timeline: frames {} to {} at {} fps",
                tl.start, tl.end, tl.fps
            );
            if !g.parameters.is_empty() {
                let vals = cooker.parameters_at(tl.start as f64)?;
                println!("parameters (at frame {}):", tl.start);
                for (k, v) in &vals {
                    let anim = if cooker.animated_parameters().contains(k) {
                        "  [animated]"
                    } else {
                        ""
                    };
                    println!("  {k:<16} {v}{anim}");
                }
            }
            println!("nodes:");
            for (name, spec) in &g.nodes {
                let anim = if cooker.is_time_dependent(name) {
                    "  [animated]"
                } else {
                    ""
                };
                let inputs = if spec.inputs.is_empty() {
                    String::new()
                } else {
                    format!(" <- {}", spec.inputs.join(", "))
                };
                let out = if *name == g.output { "  (output)" } else { "" };
                println!("  {name:<16} {:<10}{inputs}{anim}{out}", spec.kind);
            }
            let geo = cooker.cook_output(tl.start as f64)?;
            println!("output at frame {}:", tl.start);
            print_stats(&geo, "  ");
            Ok(())
        }
        Cmd::Render {
            graph,
            out,
            frame,
            all,
            width,
            height,
            yaw,
            pitch,
            wireframe,
            no_grid,
            set,
        } => {
            let settings = aitelier_render::RenderSettings {
                width,
                height,
                supersample: 2,
                wireframe,
                grid: !no_grid,
                ..Default::default()
            };
            render_cmd(
                registry, &graph, &out, frame, all, settings, yaw, pitch, &set,
            )
        }
        Cmd::Cook {
            graph,
            out,
            frame,
            range,
            all,
            node,
            set,
        } => {
            let g = load(&graph, &set)?;
            let mut cooker = Cooker::new(Arc::new(g.clone()), registry.clone())?;
            let target = node.unwrap_or_else(|| g.output.clone());
            let frames: Vec<f64> = if all {
                g.timeline.frames().map(|f| f as f64).collect()
            } else if let Some(r) = range {
                let (a, b) = (r[0].round() as i64, r[1].round() as i64);
                if b < a {
                    return Err("range END must be >= START".into());
                }
                (a..=b).map(|f| f as f64).collect()
            } else {
                vec![frame.unwrap_or(g.timeline.start as f64)]
            };
            let pattern = out.to_string_lossy().to_string();
            if frames.len() > 1 && !pattern.contains('#') {
                return Err(
                    "several frames need #### in the output name, e.g. out/vase_####.obj".into(),
                );
            }
            if !pattern.to_lowercase().ends_with(".obj") {
                return Err("only .obj output is supported in this version".into());
            }
            for f in frames {
                let geo = cooker.cook(&target, f)?;
                let path = frame_path(&pattern, f);
                if let Some(dir) = Path::new(&path).parent() {
                    if !dir.as_os_str().is_empty() {
                        fs::create_dir_all(dir)?;
                    }
                }
                fs::write(&path, obj::to_obj_string(&geo))?;
                let topo = geo.topology();
                println!(
                    "frame {f}: {} points, {} prims -> {path}{}",
                    geo.num_points(),
                    geo.num_prims(),
                    if topo.is_watertight() {
                        "  (watertight)"
                    } else {
                        ""
                    }
                );
            }
            Ok(())
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn render_cmd(
    registry: Arc<Registry>,
    graph: &Path,
    out: &Path,
    frame: Option<f64>,
    all: bool,
    settings: aitelier_render::RenderSettings,
    yaw: f32,
    pitch: f32,
    set: &[String],
) -> AnyResult<()> {
    let g = load(graph, set)?;
    let mut cooker = Cooker::new(Arc::new(g.clone()), registry)?;
    let frames: Vec<f64> = if all {
        g.timeline.frames().map(|f| f as f64).collect()
    } else {
        vec![frame.unwrap_or(g.timeline.start as f64)]
    };
    let pattern = out.to_string_lossy().to_string();
    if frames.len() > 1 && !pattern.contains('#') {
        return Err("several frames need #### in the output name, e.g. out/vase_####.png".into());
    }
    // Frame the camera once (on the first frame) so an animation does not jump around.
    let first = cooker.cook_output(frames[0])?;
    let (lo, hi) = first.bounds();
    let mut cam = aitelier_render::Camera {
        yaw,
        pitch,
        ..Default::default()
    };
    cam.frame_bounds(lo, hi);
    for f in frames {
        let geo = cooker.cook_output(f)?;
        let img = aitelier_render::render(&geo, &cam, &settings);
        let path = frame_path(&pattern, f);
        if let Some(dir) = Path::new(&path).parent() {
            if !dir.as_os_str().is_empty() {
                fs::create_dir_all(dir)?;
            }
        }
        img.write_png(&path)?;
        println!("frame {f} -> {path}");
    }
    Ok(())
}

/// Load a graph file and apply `--set NAME=VALUE` overrides.
fn load(path: &Path, sets: &[String]) -> AnyResult<Graph> {
    let text =
        fs::read_to_string(path).map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    let mut g = Graph::from_json(&text)?;
    for s in sets {
        let (k, v) = s
            .split_once('=')
            .ok_or_else(|| format!("--set expects NAME=VALUE, got `{s}`"))?;
        let value: Value = serde_json::from_str(v).unwrap_or_else(|_| Value::Str(v.to_string()));
        g.set_parameter(k.trim(), value)?;
    }
    Ok(g)
}

/// Replace a run of '#' with the zero-padded frame number.
fn frame_path(pattern: &str, frame: f64) -> String {
    let Some(start) = pattern.find('#') else {
        return pattern.to_string();
    };
    let len = pattern[start..].chars().take_while(|&c| c == '#').count();
    let num = if frame.fract() == 0.0 {
        format!("{:0len$}", frame as i64)
    } else {
        format!("{frame}")
    };
    format!("{}{}{}", &pattern[..start], num, &pattern[start + len..])
}

fn print_stats(geo: &Geometry, indent: &str) {
    let (lo, hi) = geo.bounds();
    let topo = geo.topology();
    println!(
        "{indent}points {}, primitives {}, vertices {}",
        geo.num_points(),
        geo.num_prims(),
        geo.num_vertices()
    );
    println!(
        "{indent}bounds  min [{:.3}, {:.3}, {:.3}]  max [{:.3}, {:.3}, {:.3}]",
        lo.x, lo.y, lo.z, hi.x, hi.y, hi.z
    );
    if topo.is_watertight() {
        println!("{indent}watertight: yes, volume {:.4}", geo.signed_volume());
    } else {
        println!(
            "{indent}watertight: no ({} open edges, {} non-manifold, {} flipped)",
            topo.boundary_edges, topo.nonmanifold_edges, topo.flipped_edges
        );
    }
    let attrs = |m: &std::collections::BTreeMap<String, aitelier_geo::AttribArray>| {
        m.keys().cloned().collect::<Vec<_>>().join(", ")
    };
    println!(
        "{indent}attributes: point [{}] vertex [{}] prim [{}]",
        attrs(&geo.point_attribs),
        attrs(&geo.vertex_attribs),
        attrs(&geo.prim_attribs)
    );
}

fn print_nodes(reg: &Registry, json: bool) -> AnyResult<()> {
    if json {
        let list: Vec<serde_json::Value> = reg
            .iter()
            .map(|t| {
                serde_json::json!({
                    "type": t.type_name(),
                    "description": t.description(),
                    "inputs": { "min": t.input_count().0, "max": t.input_count().1 },
                    "params": t.params(),
                })
            })
            .collect();
        println!("{}", serde_json::to_string_pretty(&list)?);
        return Ok(());
    }
    for t in reg.iter() {
        let (min, max) = t.input_count();
        let inputs = match max {
            Some(0) => "no inputs".to_string(),
            Some(m) if m == min => format!("{min} input"),
            Some(m) => format!("{min}-{m} inputs"),
            None => format!("{min}+ inputs"),
        };
        println!("{}  ({inputs})\n  {}", t.type_name(), t.description());
        for p in t.params() {
            let default = serde_json::to_string(&p.default)?;
            let opts = if p.options.is_empty() {
                String::new()
            } else {
                format!(" options: {}", p.options.join("|"))
            };
            println!(
                "    {:<14} {:<10} default {:<18} {}{}",
                p.name,
                format!("{:?}", p.kind).to_lowercase(),
                default,
                p.doc,
                opts
            );
        }
        println!();
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::frame_path;

    #[test]
    fn frame_names() {
        assert_eq!(frame_path("out/v_####.obj", 7.0), "out/v_0007.obj");
        assert_eq!(frame_path("v_#.obj", 12.0), "v_12.obj");
        assert_eq!(frame_path("v.obj", 3.0), "v.obj");
    }
}
