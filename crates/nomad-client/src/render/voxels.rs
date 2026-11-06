//! Opaque station voxel pass (phase-1 doc 01 §2, §5; doc 05 `render/voxels.rs`). The shader is
//! `shaders/voxels.wgsl`.
//!
//! Every module of `station_scene.json` is greedy-meshed once at start-up into one static vertex
//! and index buffer. Vertices keep integer world coordinates; the z scale (0.8165, or 1 with
//! `projection.true_cubes`) is applied by the world → clip matrix (doc 05).
//!
//! The palette lives in a small texture rather than the storage buffer doc 05 suggests: the device
//! is created with WebGL2-level limits so the GLES fallback keeps working, and those allow no
//! storage buffers (doc 05's module layout allows either; phase-1 doc 07 item 17).
//!
//! # Emissive output and hover (M3)
//!
//! Emissive palette entries also write their albedo into the scene pass's emissive target, which
//! feeds the bloom (`render::bloom`); every other face writes black there, so an occluded emitter
//! does not glow through. Each vertex carries its module's index, so the hovered module can be
//! drawn brighter (doc 06 M3) without a second draw.
//!
//! # No back-face culling
//!
//! Greedy merging leaves T-junctions, where one long quad edge meets the corners of several
//! shorter quads. At the snap views the dimetric projection puts every corner on the rasteriser's
//! sub-pixel grid, so these edges stay watertight; at the intermediate yaws of a turn they can
//! open single-sample pinholes (about 200 per sweep of views and zooms in the
//! `station_has_no_holes` measurement). Every module is a closed surface, so a ray through such a
//! pinhole still meets the module's far side. Drawing back faces therefore fills every pinhole
//! with the station's own colours instead of the background, for a few thousand extra triangles
//! that the depth test mostly rejects.

use bytemuck::{Pod, Zeroable};
use wgpu::util::DeviceExt;

use super::context::TargetFormat;
use crate::assets::voxel::{Chunk, PaletteEntry, StationScene};
use crate::camera::ViewTransform;
use crate::mesh::greedy::{Quad, greedy_mesh};

/// Palette texels per texture row (`PALETTE_WIDTH` in the shader).
pub const PALETTE_WIDTH: u32 = 256;

/// Voxel triangles are wound clockwise on screen when seen from outside. Quads are wound
/// counter-clockwise by the right-hand rule in world space, and the doc 01 projection mirrors the
/// world (x runs right-down and y left-down on screen while z is up), so their screen winding is
/// clockwise. Checked by `visible_faces_are_wound_clockwise_on_screen`. Culling is off (module
/// docs), so this matters only if it is ever turned on.
const FRONT_FACE: wgpu::FrontFace = wgpu::FrontFace::Cw;

/// One mesh vertex: integer world position, module index and (palette index, face id). 12 bytes.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Pod, Zeroable)]
pub struct VoxelVertex {
    /// x, y, z and the index of the vertex's module in the scene (for the hover highlight; the
    /// loader caps the module count far below `i16::MAX`).
    pub position: [i16; 4],
    pub palette_face: [u16; 2],
}

/// The station meshed into one vertex and index list.
pub struct StationMesh {
    pub vertices: Vec<VoxelVertex>,
    pub indices: Vec<u32>,
    pub quad_count: usize,
}

/// Greedy-meshes every module (doc 03: each module separately).
pub fn build_station_mesh(scene: &StationScene) -> StationMesh {
    build_mesh(scene, greedy_mesh)
}

/// The station with one quad per exposed face (`mesh::greedy::unmerged_mesh`), for GPU tests.
#[cfg(test)]
pub fn build_unmerged_station_mesh(scene: &StationScene) -> StationMesh {
    build_mesh(scene, crate::mesh::greedy::unmerged_mesh)
}

fn build_mesh(scene: &StationScene, mesher: fn(&Chunk) -> Vec<Quad>) -> StationMesh {
    let mut mesh = StationMesh {
        vertices: Vec::new(),
        indices: Vec::new(),
        quad_count: 0,
    };
    for (index, module) in scene.modules.iter().enumerate() {
        let quads = mesher(&module.chunk);
        tracing::debug!(
            module = %module.id,
            voxels = module.chunk.voxels().count(),
            quads = quads.len(),
            "meshed station module"
        );
        let module_index = i16::try_from(index)
            .expect("invariant: the loader caps the module count below i16::MAX");
        for quad in &quads {
            push_quad(&mut mesh, quad, module_index);
        }
        mesh.quad_count += quads.len();
    }
    mesh
}

fn push_quad(mesh: &mut StationMesh, quad: &Quad, module_index: i16) {
    let base = u32::try_from(mesh.vertices.len())
        .expect("invariant: the validated scene has far fewer than 2^32 vertices");
    let coord =
        |v: i32| i16::try_from(v).expect("invariant: voxel coordinates are validated to fit i16");
    for corner in quad.corners {
        mesh.vertices.push(VoxelVertex {
            position: [
                coord(corner[0]),
                coord(corner[1]),
                coord(corner[2]),
                module_index,
            ],
            palette_face: [quad.palette_index, quad.face.id()],
        });
    }
    mesh.indices
        .extend([base, base + 1, base + 2, base, base + 2, base + 3]);
}

/// RGBA8 texels for the palette texture: sRGB albedo and alpha 255 for emissive entries, padded to
/// whole rows of [`PALETTE_WIDTH`]. Returns the texels and the number of rows.
pub fn palette_texels(palette: &[PaletteEntry]) -> (Vec<u8>, u32) {
    let width = PALETTE_WIDTH as usize;
    let rows = palette.len().div_ceil(width).max(1);
    let mut texels = vec![0; rows * width * 4];
    for (texel, entry) in texels.chunks_exact_mut(4).zip(palette) {
        texel.copy_from_slice(&[
            entry.rgb.r,
            entry.rgb.g,
            entry.rgb.b,
            if entry.emissive { 255 } else { 0 },
        ]);
    }
    let rows = u32::try_from(rows).expect("invariant: palettes hold at most 65535 entries");
    (texels, rows)
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
pub struct VoxelUniforms {
    view_proj: [[f32; 4]; 4],
    view_right: [f32; 4],
    shading: [f32; 4],
    hover: [f32; 4],
}

impl VoxelUniforms {
    /// `hovered` is the index of the module under the cursor; `brightness_lift` comes from
    /// `scene_config.json` → `hover`.
    pub fn new(
        view: &ViewTransform,
        shading: [f32; 3],
        hovered: Option<usize>,
        brightness_lift: f32,
    ) -> Self {
        // Module indices are below MAX_MODULES, so they are exact in f32; -1 matches no module.
        let hovered = hovered.map_or(-1.0, |index| index as f32);
        Self {
            view_proj: view.view_proj().to_cols_array_2d(),
            view_right: [view.right.x, view.right.y, 0.0, 0.0],
            shading: [shading[0], shading[1], shading[2], 0.0],
            hover: [hovered, brightness_lift, 0.0, 0.0],
        }
    }
}

struct MeshBuffers {
    vertices: wgpu::Buffer,
    indices: wgpu::Buffer,
    index_count: u32,
}

pub struct VoxelPass {
    uniform_buffer: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
    pipeline: wgpu::RenderPipeline,
    /// `None` for a scene without voxels; wgpu cannot bind empty buffers.
    mesh: Option<MeshBuffers>,
    /// Top, left-facing and right-facing factors from the scene file.
    shading: [f32; 3],
}

impl VoxelPass {
    /// Greedy-meshes the station and sets up the pass.
    pub fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        target: TargetFormat,
        scene: &StationScene,
    ) -> Self {
        let station = build_station_mesh(scene);
        tracing::info!(
            modules = scene.modules.len(),
            quads = station.quad_count,
            vertices = station.vertices.len(),
            "station meshed"
        );
        Self::from_mesh(device, queue, target, scene, &station)
    }

    /// The pass for an already meshed station; palette and shading come from `scene`.
    pub fn from_mesh(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        target: TargetFormat,
        scene: &StationScene,
        station: &StationMesh,
    ) -> Self {
        let mesh = MeshBuffers::new(device, station);
        let palette_view = palette_texture(device, queue, &scene.palette)
            .create_view(&wgpu::TextureViewDescriptor::default());
        let uniform_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("voxel uniforms"),
            contents: bytemuck::bytes_of(&VoxelUniforms::zeroed()),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("voxels"),
            entries: &[
                super::uniform_entry(wgpu::ShaderStages::VERTEX, size_of::<VoxelUniforms>()),
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::VERTEX,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: false },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
            ],
        });
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("voxels"),
            layout: &layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: uniform_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&palette_view),
                },
            ],
        });
        Self {
            uniform_buffer,
            bind_group,
            pipeline: voxel_pipeline(device, &layout, target),
            mesh,
            shading: [scene.shading.top, scene.shading.left, scene.shading.right],
        }
    }

    /// Uploads the frame's uniforms. `hovered` (a module index) is drawn with its non-emissive
    /// faces lifted by `1 + brightness_lift`.
    pub fn prepare(
        &self,
        queue: &wgpu::Queue,
        view: &ViewTransform,
        hovered: Option<usize>,
        brightness_lift: f32,
    ) {
        let uniforms = VoxelUniforms::new(view, self.shading, hovered, brightness_lift);
        queue.write_buffer(&self.uniform_buffer, 0, bytemuck::bytes_of(&uniforms));
    }

    pub fn draw(&self, pass: &mut wgpu::RenderPass<'_>) {
        let Some(mesh) = &self.mesh else {
            return;
        };
        pass.set_bind_group(0, &self.bind_group, &[]);
        pass.set_pipeline(&self.pipeline);
        pass.set_vertex_buffer(0, mesh.vertices.slice(..));
        pass.set_index_buffer(mesh.indices.slice(..), wgpu::IndexFormat::Uint32);
        pass.draw_indexed(0..mesh.index_count, 0, 0..1);
    }
}

impl MeshBuffers {
    /// Uploads a mesh; `None` when it is empty.
    fn new(device: &wgpu::Device, station: &StationMesh) -> Option<Self> {
        (!station.indices.is_empty()).then(|| Self {
            vertices: device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("station vertices"),
                contents: bytemuck::cast_slice(&station.vertices),
                usage: wgpu::BufferUsages::VERTEX,
            }),
            indices: device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("station indices"),
                contents: bytemuck::cast_slice(&station.indices),
                usage: wgpu::BufferUsages::INDEX,
            }),
            index_count: u32::try_from(station.indices.len())
                .expect("invariant: the validated scene has far fewer than 2^32 indices"),
        })
    }
}

fn palette_texture(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    palette: &[PaletteEntry],
) -> wgpu::Texture {
    let (texels, rows) = palette_texels(palette);
    device.create_texture_with_data(
        queue,
        &wgpu::TextureDescriptor {
            label: Some("palette"),
            size: wgpu::Extent3d {
                width: PALETTE_WIDTH,
                height: rows,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        },
        wgpu::util::TextureDataOrder::LayerMajor,
        &texels,
    )
}

fn voxel_pipeline(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    target: TargetFormat,
) -> wgpu::RenderPipeline {
    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("voxels"),
        bind_group_layouts: &[Some(layout)],
        immediate_size: 0,
    });
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("voxels.wgsl"),
        source: wgpu::ShaderSource::Wgsl(super::VOXELS_WGSL.into()),
    });
    // Opaque: both targets are overwritten, the emissive one with the albedo of emissive faces
    // and black elsewhere.
    let targets = super::scene_targets(target, None);
    let attributes = wgpu::vertex_attr_array![0 => Sint16x4, 1 => Uint16x2];
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("voxels"),
        layout: Some(&pipeline_layout),
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: Some("vs_voxel"),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            buffers: &[Some(wgpu::VertexBufferLayout {
                array_stride: size_of::<VoxelVertex>() as wgpu::BufferAddress,
                step_mode: wgpu::VertexStepMode::Vertex,
                attributes: &attributes,
            })],
        },
        primitive: wgpu::PrimitiveState {
            topology: wgpu::PrimitiveTopology::TriangleList,
            front_face: FRONT_FACE,
            // Back faces fill T-junction pinholes (module docs).
            cull_mode: None,
            ..Default::default()
        },
        // Opaque: nearer is smaller depth (camera::ViewTransform::view_proj).
        depth_stencil: Some(super::context::depth_stencil(
            true,
            Some(wgpu::CompareFunction::Less),
        )),
        multisample: target.multisample(),
        fragment: Some(wgpu::FragmentState {
            module: &shader,
            entry_point: Some("fs_voxel"),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            targets: &targets,
        }),
        multiview_mask: None,
        cache: None,
    })
}

#[cfg(test)]
mod tests {
    use glam::{Vec2, Vec3};

    use super::*;
    use crate::assets::voxel::load_station_scene;
    use crate::camera::{Camera, Projection};
    use crate::config::{
        default_asset_root, load_scene_config, scene_config_path, station_scene_path,
    };
    use crate::mesh::greedy::Face;

    fn ndc(view: &ViewTransform, p: [i32; 3]) -> Vec2 {
        let p = Vec3::new(p[0] as f32, p[1] as f32, p[2] as f32);
        view.view_proj().project_point3(p).truncate()
    }

    /// Twice the signed area in NDC (y up): positive when counter-clockwise.
    fn signed_area(points: &[Vec2]) -> f32 {
        (0..points.len())
            .map(|i| points[i].perp_dot(points[(i + 1) % points.len()]))
            .sum()
    }

    /// The faces a camera sees (top and the two sides facing it) come out wound as
    /// [`FRONT_FACE`] in every view and the hidden ones the other way: the mesher's outward
    /// winding and the projection agree.
    #[test]
    fn visible_faces_are_wound_clockwise_on_screen() {
        assert_eq!(FRONT_FACE, wgpu::FrontFace::Cw);
        let config = load_scene_config(&scene_config_path(&default_asset_root())).unwrap();
        let mut camera = Camera::new(Projection::from_config(&config), Vec2::new(1920.0, 1080.0));
        let text = serde_json::json!({
            "format": "voxel-rts/scene@1",
            "shading": { "top": 1.0, "left": 0.77, "right": 0.48 },
            "palette": [{ "rgb": "#858c9a", "emissive": false }],
            "modules": [{ "id": "cube", "bounds": { "min": [3, -4, 2], "max": [3, -4, 2] }, "voxels": [[3, -4, 2, 0]] }]
        })
        .to_string();
        let scene = StationScene::from_json(&text, std::path::Path::new("test.json")).unwrap();
        let quads = greedy_mesh(&scene.modules[0].chunk);
        assert_eq!(quads.len(), 6);
        for view_index in 0..4 {
            camera.set_view(view_index);
            let view = camera.transform();
            // The camera looks along +up_ground and down: faces whose normal points against that
            // are visible.
            let toward_viewer = |face: Face| {
                let n = face.normal();
                let horizontal = Vec2::new(n[0] as f32, n[1] as f32).dot(view.up_ground);
                n[2] > 0 || horizontal < -1e-3
            };
            for quad in &quads {
                let area = signed_area(&quad.corners.map(|c| ndc(&view, c)));
                if toward_viewer(quad.face) {
                    assert!(
                        area < 0.0,
                        "view {view_index}: visible {:?} is not clockwise",
                        quad.face
                    );
                } else {
                    assert!(
                        area >= 0.0,
                        "view {view_index}: hidden {:?} would not be culled",
                        quad.face
                    );
                }
            }
        }
    }

    #[test]
    fn station_mesh_is_compact_and_in_range() {
        let scene = load_station_scene(&station_scene_path(&default_asset_root())).unwrap();
        let mesh = build_station_mesh(&scene);
        assert_eq!(mesh.vertices.len(), mesh.quad_count * 4);
        assert_eq!(mesh.indices.len(), mesh.quad_count * 6);
        // Doc 05: about 17k voxels become about 20k faces after greedy meshing.
        assert!(
            mesh.quad_count > 10_000 && mesh.quad_count < 25_000,
            "{} quads",
            mesh.quad_count
        );
        let palette_len = scene.palette.len();
        assert!(
            mesh.vertices
                .iter()
                .all(|v| usize::from(v.palette_face[0]) < palette_len && v.palette_face[1] < 6)
        );
        assert!(
            mesh.indices
                .iter()
                .all(|&i| (i as usize) < mesh.vertices.len())
        );
        // The fourth lane names the vertex's module, for the hover highlight; every module that
        // has faces appears, in scene order.
        let mut modules: Vec<i16> = mesh.vertices.iter().map(|v| v.position[3]).collect();
        assert!(modules.is_sorted());
        modules.dedup();
        let expected: Vec<i16> = (0..scene.modules.len())
            .map(|i| i16::try_from(i).unwrap())
            .collect();
        assert_eq!(modules, expected);
    }

    #[test]
    fn palette_texels_mark_emissive_entries() {
        let scene = load_station_scene(&station_scene_path(&default_asset_root())).unwrap();
        let (texels, rows) = palette_texels(&scene.palette);
        assert_eq!(rows, 2);
        assert_eq!(texels.len(), 2 * 256 * 4);
        for (i, entry) in scene.palette.iter().enumerate() {
            let t = &texels[i * 4..i * 4 + 4];
            assert_eq!(&t[..3], &[entry.rgb.r, entry.rgb.g, entry.rgb.b]);
            assert_eq!(t[3] == 255, entry.emissive);
        }
        assert_eq!(texels.chunks_exact(4).filter(|t| t[3] == 255).count(), 5);
    }

    #[test]
    fn vertex_and_uniform_layouts_match_the_shader() {
        assert_eq!(size_of::<VoxelVertex>(), 12);
        assert_eq!(size_of::<VoxelUniforms>(), 64 + 3 * 16);
    }
}
