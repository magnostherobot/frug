//! FRUG is intended to provide a similar abstraction layer over graphics programming as to how SDL does for C++, meaning that it should provide developers enough control and flexibility to implement their own architectures & design patterns, yet simplifying the process of working with graphics so developers won't have to worry about implementing all the repetitive tasks related to getting things to the screen.
//!
//! Please see [the documentation](https://santyarellano.github.io/frug_book/) for more information.

use cgmath::Matrix4;
// Exported libs
pub use winit::event::{KeyboardInput, VirtualKeyCode};
pub use winit::event_loop::EventLoop;
pub use winit_input_helper::WinitInputHelper as InputHelper;

// Internal use
use wgpu::util::DeviceExt;
use winit::{
    dpi::LogicalSize,
    event::{Event, WindowEvent},
    event_loop::ControlFlow,
    window::Window,
};

#[cfg(target_os = "macos")]
use winit::platform::macos::WindowExtMacOS;

mod texture;

/// Enum to use with `InputHelper.mouse_pressed` to detect user input via mouse.
pub enum MouseButton {
    Left,
    Right,
    Middle,
    Other,
}

impl Into<usize> for MouseButton {
    fn into(self) -> usize {
        self as usize
    }
}

#[rustfmt::skip]
pub const OPENGL_TO_WGPU_MATRIX: cgmath::Matrix4<f32> = cgmath::Matrix4::new(
    1.0, 0.0, 0.0, 0.0, 
    0.0, 1.0, 0.0, 0.0,
    0.0, 0.0, 0.5, 0.0, 
    0.0, 0.0, 0.5, 1.0
);

/// Struct that defines the properties of our camera.
/// The main components in here are:
/// `eye (cgmath::Point3<f32>)`     - specifies where our camera is looking from.
/// `target (cgmath::Point3<f32>)`  - specifies where our camera is looking at.
pub struct Camera {
    pub eye: cgmath::Point3<f32>,
    pub target: cgmath::Point3<f32>,
    pub up: cgmath::Vector3<f32>,
    pub aspect: f32,
    fovy: f32,
    znear: f32,
    zfar: f32,
    is_perspective: bool,
}

impl Camera {
    fn build_view_projection_matrix(&self) -> cgmath::Matrix4<f32> {
        let view = cgmath::Matrix4::look_at_rh(self.eye, self.target, self.up);

        let proj: Matrix4<f32>;
        if self.is_perspective {
            proj = cgmath::perspective(cgmath::Deg(self.fovy), self.aspect, self.znear, self.zfar);
        } else {
            proj = cgmath::ortho(-1.0, 1.0, -1.0, 1.0, self.znear, self.zfar);
        }

        return OPENGL_TO_WGPU_MATRIX * proj * view;
    }
}

/// Our camera uniform to store the view projection matrix.
#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
struct CameraUniform {
    view_proj: [[f32; 4]; 4],
}

impl CameraUniform {
    fn new() -> Self {
        use cgmath::SquareMatrix;
        Self {
            view_proj: cgmath::Matrix4::identity().into(),
        }
    }

    fn update_view_proj(&mut self, camera: &Camera) {
        self.view_proj = camera.build_view_projection_matrix().into();
    }
}

/// Vertex struct
#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Vertex {
    pub position: [f32; 3],
    pub text_coords: [f32; 2],
    pub color: [f32; 3],
}

impl Default for Vertex {
    fn default() -> Self {
        Vertex {
            position: [0.0, 0.0, 0.0],
            text_coords: [0.0, 0.0],
            color: [1.0, 1.0, 1.0],
        }
    }
}

/// Implementation of Vertex methods
impl Vertex {
    fn desc<'a>() -> wgpu::VertexBufferLayout<'a> {
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Vertex>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &[
                // position
                wgpu::VertexAttribute {
                    offset: 0,
                    shader_location: 0,
                    format: wgpu::VertexFormat::Float32x3,
                },
                // text coords
                wgpu::VertexAttribute {
                    offset: std::mem::size_of::<[f32; 3]>() as wgpu::BufferAddress,
                    shader_location: 1,
                    format: wgpu::VertexFormat::Float32x2,
                },
                // color
                wgpu::VertexAttribute {
                    offset: std::mem::size_of::<[f32; 5]>() as wgpu::BufferAddress,
                    shader_location: 2,
                    format: wgpu::VertexFormat::Float32x3,
                },
            ],
        }
    }
}

/// Drawable Object struct
/// Contains:
/// `indices_low_pos (u32)` - The lower bound position in the indices array.
/// `indices_hi_pos (u32)`  - The higher bound position in the indices array.
/// `bind_group_idx (u32)`  - The index of the bind group to use.
struct DrawableObj {
    indices_low_pos: u32,
    indices_hi_pos: u32,
    bind_group_idx: Option<usize>,
}

/// The Frug instance.
/// Contains the surface in which we draw, the device we're using, the queue, the surface configuration, surface size, window, background color, and render pipeline.
pub struct FrugInstance {
    surface: wgpu::Surface,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    size: winit::dpi::PhysicalSize<u32>,
    window: Window,
    background_color: wgpu::Color,
    render_pipeline_textures: wgpu::RenderPipeline,
    render_pipeline_colors: wgpu::RenderPipeline,
    vertex_buffer: wgpu::Buffer,
    index_buffer: wgpu::Buffer,
    staging_vertices: Vec<Vertex>,
    staging_indices: Vec<u16>,
    num_indices: u32,
    texture_bind_group_layout: wgpu::BindGroupLayout,
    diffuse_bind_groups: Vec<wgpu::BindGroup>,
    drawable_objects: Vec<DrawableObj>,
    pub camera: Camera,
    camera_uniform: CameraUniform,
    camera_buffer: wgpu::Buffer,
    camera_bind_group: wgpu::BindGroup,
    exit_requested: bool,
}

/// Implementation of FrugInstance methods
impl FrugInstance {
    /// Creates a new instance of FrugInstance, instantiating the window, configuration, and the surface to draw in.
    async fn new_instance(window_title: &str, event_loop: &EventLoop<()>) -> Self {
        // Enable wgpu logging
        env_logger::init();

        // Setup
        let window = Window::new(&event_loop).unwrap();
        window.set_title(window_title);
        let size = window.inner_size();
        let background_color = wgpu::Color {
            r: 0.0,
            g: 0.0,
            b: 0.0,
            a: 1.0,
        };
        let vertices: &[Vertex] = &[];
        let indices: &[u16] = &[];

        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::default());

        let surface = unsafe { instance.create_surface(&window) }.unwrap();

        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::default(),
                compatible_surface: Some(&surface),
                force_fallback_adapter: false,
            })
            .await
            .expect("Failed to find an appropiate adapter.");

        let (device, queue) = adapter
            .request_device(
                &wgpu::DeviceDescriptor {
                    label: None,
                    features: wgpu::Features::empty(),
                    limits: wgpu::Limits::default(),
                },
                None,
            )
            .await
            .expect("Failed to create device.");

        let surface_caps = surface.get_capabilities(&adapter);
        let surface_format = surface_caps
            .formats
            .iter()
            .copied()
            .filter(|f| f.describe().srgb)
            .next()
            .unwrap_or(surface_caps.formats[0]);

        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format: surface_format,
            width: size.width,
            height: size.height,
            present_mode: surface_caps.present_modes[0],
            alpha_mode: surface_caps.alpha_modes[0],
            view_formats: vec![],
        };
        surface.configure(&device, &config);

        // load texture shader (to use with textured vertices)
        let shader_texture =
            device.create_shader_module(wgpu::include_wgsl!("shader_texture.wgsl"));

        // load color shader (to use with colored vertices)
        let shader_color = device.create_shader_module(wgpu::include_wgsl!("shader_color.wgsl"));

        // Camera
        let camera = Camera {
            eye: (0.0, 0.0, 2.0).into(),
            target: (0.0, 0.0, 0.0).into(),
            up: cgmath::Vector3::unit_y(),
            aspect: config.width as f32 / config.height as f32,
            fovy: 45.0,
            znear: 0.1,
            zfar: 100.0,
            is_perspective: false,
        };

        let mut camera_uniform = CameraUniform::new();
        camera_uniform.update_view_proj(&camera);

        let camera_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Camera Buffer"),
            contents: bytemuck::cast_slice(&[camera_uniform]),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });

        let camera_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                entries: &[wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                }],
                label: Some("Camera bind group layout"),
            });

        let camera_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            layout: &camera_bind_group_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: camera_buffer.as_entire_binding(),
            }],
            label: Some("Camera bind group"),
        });

        // we use this to load textures in a render pipeline
        let texture_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("texture_bind_group_layout"),
                entries: &[
                    wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            sample_type: wgpu::TextureSampleType::Float { filterable: true },
                            view_dimension: wgpu::TextureViewDimension::D2,
                            multisampled: false,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 1,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                        count: None,
                    },
                ],
            });

        // the render pipeline layout to use with textures.
        let render_pipeline_textures_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("Render Pipeline Layout"),
                bind_group_layouts: &[&texture_bind_group_layout, &camera_bind_group_layout],
                push_constant_ranges: &[],
            });

        // the render pipeline layout to use with colors.
        let render_pipeline_colors_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("Render Pipeline Layout"),
                bind_group_layouts: &[&camera_bind_group_layout],
                push_constant_ranges: &[],
            });

        // our render pipeline to use with textures
        let render_pipeline_textures =
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("Render Pipeline Textures"),
                layout: Some(&render_pipeline_textures_layout),
                vertex: wgpu::VertexState {
                    module: &shader_texture,
                    entry_point: "vs_main",
                    buffers: &[Vertex::desc()],
                },
                fragment: Some(wgpu::FragmentState {
                    module: &shader_texture,
                    entry_point: "fs_main",
                    targets: &[Some(wgpu::ColorTargetState {
                        format: config.format,
                        blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                primitive: wgpu::PrimitiveState {
                    topology: wgpu::PrimitiveTopology::TriangleList,
                    strip_index_format: None,
                    front_face: wgpu::FrontFace::Ccw,
                    cull_mode: Some(wgpu::Face::Back),
                    unclipped_depth: false,
                    polygon_mode: wgpu::PolygonMode::Fill,
                    conservative: false,
                },
                depth_stencil: None,
                multisample: wgpu::MultisampleState {
                    count: 1,
                    mask: !0,
                    alpha_to_coverage_enabled: false,
                },
                multiview: None,
            });

        // our render pipeline to use with colors
        let render_pipeline_colors =
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("Render Pipeline Colors"),
                layout: Some(&render_pipeline_colors_layout),
                vertex: wgpu::VertexState {
                    module: &shader_color,
                    entry_point: "vs_main",
                    buffers: &[Vertex::desc()],
                },
                fragment: Some(wgpu::FragmentState {
                    module: &shader_color,
                    entry_point: "fs_main",
                    targets: &[Some(wgpu::ColorTargetState {
                        format: config.format,
                        blend: Some(wgpu::BlendState::REPLACE),
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                primitive: wgpu::PrimitiveState {
                    topology: wgpu::PrimitiveTopology::TriangleList,
                    strip_index_format: None,
                    front_face: wgpu::FrontFace::Ccw,
                    cull_mode: Some(wgpu::Face::Back),
                    unclipped_depth: false,
                    polygon_mode: wgpu::PolygonMode::Fill,
                    conservative: false,
                },
                depth_stencil: None,
                multisample: wgpu::MultisampleState {
                    count: 1,
                    mask: !0,
                    alpha_to_coverage_enabled: false,
                },
                multiview: None,
            });

        let vertex_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Vertex Buffer"),
            contents: bytemuck::cast_slice(&vertices),
            usage: wgpu::BufferUsages::VERTEX,
        });

        let index_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Index Buffer"),
            contents: bytemuck::cast_slice(&indices),
            usage: wgpu::BufferUsages::INDEX,
        });

        let num_indices = indices.len() as u32;

        Self {
            window,
            surface,
            device,
            queue,
            config,
            size,
            background_color,
            render_pipeline_textures,
            render_pipeline_colors,
            vertex_buffer,
            index_buffer,
            staging_vertices: Vec::new(),
            staging_indices: Vec::new(),
            num_indices,
            texture_bind_group_layout,
            diffuse_bind_groups: Vec::new(),
            drawable_objects: Vec::new(),
            camera,
            camera_uniform,
            camera_buffer,
            camera_bind_group,
            exit_requested: false,
        }
    }

    /// Resize the canvas for our window given a new defined size.
    fn resize(&mut self, new_size: winit::dpi::PhysicalSize<u32>) {
        if new_size.width > 0 && new_size.height > 0 {
            self.size = new_size;
            self.config.width = new_size.width;
            self.config.height = new_size.height;
            self.surface.configure(&self.device, &self.config);
        }
    }

    /// Renders all textured objects based on data on buffers.
    fn render(&mut self) -> Result<(), wgpu::SurfaceError> {
        let output = self.surface.get_current_texture()?;

        let view = output
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("Render Encoder"),
            });

        // draw our objects
        let mut render_pass_op = wgpu::LoadOp::Clear(self.background_color);
        for drawable_obj in &self.drawable_objects {
            let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Render Pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: render_pass_op,
                        store: true,
                    },
                })],
                depth_stencil_attachment: None,
            });

            let mut camera_bind_group_idx = 0;

            // texture bind group
            match drawable_obj.bind_group_idx {
                Some(idx) => {
                    render_pass.set_pipeline(&self.render_pipeline_textures);
                    render_pass.set_bind_group(0, &self.diffuse_bind_groups[idx], &[]);

                    // update to camera bind group index so it is in the correct binding position
                    camera_bind_group_idx = 1;
                }
                None => {
                    // We'll use the render pipeline with colors instead of textures
                    render_pass.set_pipeline(&self.render_pipeline_colors);
                }
            }

            // camera bind group
            render_pass.set_bind_group(camera_bind_group_idx, &self.camera_bind_group, &[]);

            render_pass.set_vertex_buffer(0, self.vertex_buffer.slice(..));
            render_pass.set_index_buffer(self.index_buffer.slice(..), wgpu::IndexFormat::Uint16);

            render_pass.draw_indexed(
                drawable_obj.indices_low_pos..drawable_obj.indices_hi_pos,
                0,
                0..1,
            );

            render_pass_op = wgpu::LoadOp::Load;
        }

        // Clear objects
        self.drawable_objects.clear();

        // submit the encoder to the queue & present it on the screen
        self.queue.submit(std::iter::once(encoder.finish()));
        output.present();

        Ok(())
    }

    /// Updates some feaures of our frug instance, like the camera.
    fn update(&mut self) {
        self.camera_uniform.update_view_proj(&self.camera);
        self.queue.write_buffer(
            &self.camera_buffer,
            0,
            bytemuck::cast_slice(&[self.camera_uniform]),
        );
    }

    /// Signify that the event loop should be exited when next possible.
    pub fn exit(&mut self) {
        self.exit_requested = true;
    }

    /// Sets new background color.
    ///
    /// Receives a wgpu color (you can create one using the `frug::create_color` method).
    ///
    /// # Example
    /// ```
    /// let new_color = frug::create_color(0.2, 0.3, 0.4, 1.0);
    /// my_frug_instance.set_background_color(new_color);
    /// ```
    pub fn set_background_color(&mut self, color: wgpu::Color) {
        self.background_color = color;
    }

    /// Sets or exists fullscreen mode.
    /// Receives a bool:
    /// `true`  - Sets fullscreen.
    /// `false` - Exits fullscreen.
    pub fn set_fullscreen(&mut self, fullscreen: bool) {
        if fullscreen {
            // Enter fullscreen
            #[cfg(target_os = "macos")]
            if !self.window.simple_fullscreen() {
                self.window.set_simple_fullscreen(true);
            }

            #[cfg(not(target_os = "macos"))]
            if !self.window.fullscreen().is_some() {
                let fscreen = Some(winit::window::Fullscreen::Borderless(None));
                self.window.set_fullscreen(fscreen);
            }
        } else {
            // Exit fullscreen
            #[cfg(target_os = "macos")]
            if self.window.simple_fullscreen() {
                self.window.set_simple_fullscreen(false);
            }

            #[cfg(not(target_os = "macos"))]
            if self.window.fullscreen().is_some() {
                self.window.set_fullscreen(None);
            }
        }
    }

    /// Sets the window size.
    /// Receives both the width and height of the new size we want for our window.
    pub fn set_window_size(&mut self, width: f32, height: f32) {
        self.window.set_inner_size(LogicalSize::new(width, height));
    }

    /// Updates the vertex and index buffers with the staging data.
    pub fn update_buffers(&mut self) {
        self.vertex_buffer = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("Vertex Buffer"),
                contents: bytemuck::cast_slice(&self.staging_vertices),
                usage: wgpu::BufferUsages::VERTEX,
            });

        self.index_buffer = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("Index Buffer"),
                contents: bytemuck::cast_slice(&self.staging_indices),
                usage: wgpu::BufferUsages::INDEX,
            });

        self.num_indices = self.staging_indices.len() as u32;
    }

    /// Adds a set of vertices and indices to the staging data.
    pub fn add_colored_vertices(&mut self, vertices: &[Vertex], indices: &[u16]) {
        // Add the vertices to the drawable objects vector
        let low_bound = self.staging_indices.len() as u32;
        self.drawable_objects.push(DrawableObj {
            indices_low_pos: low_bound,
            indices_hi_pos: low_bound + indices.len() as u32,
            bind_group_idx: None,
        });

        self.add_staging_indexed_vertices(vertices, indices);
    }

    /// Adds a set of vertices and indices to the staging data.
    fn add_staging_indexed_vertices(&mut self, vertices: &[Vertex], indices: &[u16]) {
        // update the indices to match the number of current vertices
        let offset: u16 = self.staging_vertices.len() as u16;
        for index in indices {
            self.staging_indices.push(index + offset);
        }

        self.staging_vertices.extend(vertices);
    }

    /// Clears the staging buffers data so the next frame is empty.
    pub fn clear(&mut self) {
        self.staging_vertices.clear();
        self.staging_indices.clear();
    }

    /// Adds a rectangle to the staging data using a texture.
    /// Receives:
    /// * `x (f32)`             - The x origin of the rectangle.
    /// * `y (f32)`             - The y origin of the rectangle.
    /// * `w (f32)`             - The width of the rectangle.
    /// * `h (f32)`             - The height of the rectangle.
    /// * `texture_index (u16)` - The index of the texture we're drawing.
    pub fn add_tex_rect(
        &mut self,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        texture_index: usize,
        flip_x: bool,
        flip_y: bool,
    ) {
        // Add the object to the drawable objects vector
        let low_bound = self.staging_indices.len() as u32;
        self.drawable_objects.push(DrawableObj {
            indices_low_pos: low_bound,
            indices_hi_pos: low_bound + 6,
            bind_group_idx: Some(texture_index),
        });

        let mut tex_coords = [
            0.0, // left
            1.0, // right
            0.0, // top
            1.0, // botom
        ];

        if flip_x {
            tex_coords[0] = 1.0;
            tex_coords[1] = 0.0;
        }

        if flip_y {
            tex_coords[2] = 1.0;
            tex_coords[3] = 0.0;
        }

        self.add_staging_indexed_vertices(
            &[
                Vertex {
                    position: [x, y, 0.0],
                    text_coords: [tex_coords[0], tex_coords[2]],
                    ..Default::default()
                },
                Vertex {
                    position: [x, y - h, 0.0],
                    text_coords: [tex_coords[0], tex_coords[3]],
                    ..Default::default()
                },
                Vertex {
                    position: [x + w, y - h, 0.0],
                    text_coords: [tex_coords[1], tex_coords[3]],
                    ..Default::default()
                },
                Vertex {
                    position: [x + w, y, 0.0],
                    text_coords: [tex_coords[1], tex_coords[2]],
                    ..Default::default()
                },
            ],
            &[0, 1, 3, 1, 2, 3],
        );
    }

    /// Adds a rectangle to the staging data using a texture.
    /// Receives:
    /// * `x (f32)`             - The x origin of the rectangle.
    /// * `y (f32)`             - The y origin of the rectangle.
    /// * `w (f32)`             - The width of the rectangle.
    /// * `h (f32)`             - The height of the rectangle.
    /// * `color [f32; 3]`      - The [red, green, blue] definition of the color to use.
    pub fn add_colored_rect(&mut self, x: f32, y: f32, w: f32, h: f32, color: [f32; 3]) {
        // Add the object to the drawable objects vector
        let low_bound = self.staging_indices.len() as u32;
        self.drawable_objects.push(DrawableObj {
            indices_low_pos: low_bound,
            indices_hi_pos: low_bound + 6,
            bind_group_idx: None,
        });

        self.add_staging_indexed_vertices(
            &[
                Vertex {
                    position: [x, y, 0.0],
                    color,
                    ..Default::default()
                },
                Vertex {
                    position: [x, y - h, 0.0],
                    color,
                    ..Default::default()
                },
                Vertex {
                    position: [x + w, y - h, 0.0],
                    color,
                    ..Default::default()
                },
                Vertex {
                    position: [x + w, y, 0.0],
                    color,
                    ..Default::default()
                },
            ],
            &[0, 1, 3, 1, 2, 3],
        );
    }

    /// Loads a texture
    pub fn load_texture(&mut self, img_bytes: &[u8]) -> usize {
        let diffuse_texture =
            texture::Texture::from_bytes(&self.device, &self.queue, img_bytes, "texture").unwrap();

        let diffuse_bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("diffuse_bind_group"),
            layout: &self.texture_bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&diffuse_texture.view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&diffuse_texture.sampler),
                },
            ],
        });

        self.diffuse_bind_groups.push(diffuse_bind_group);

        return self.diffuse_bind_groups.len() - 1;
    }

    /// Starts running the loop
    pub fn run<F: 'static + FnMut(&mut FrugInstance, &InputHelper)>(
        mut self,
        event_loop: EventLoop<()>,
        mut update_function: F,
    ) {
        let mut input = winit_input_helper::WinitInputHelper::new();

        // Run the loop
        event_loop.run(move |event, _, control_flow| {
            *control_flow = ControlFlow::Wait;

            input.update(&event);

            // Act on events
            match event {
                Event::WindowEvent {
                    ref event,
                    window_id,
                } if window_id == self.window.id() => match event {
                    // Window events
                    // Close
                    WindowEvent::CloseRequested => {
                        *control_flow = ControlFlow::Exit;
                    }

                    // Resize
                    WindowEvent::Resized(physical_size) => {
                        self.resize(*physical_size);
                    }
                    WindowEvent::ScaleFactorChanged { new_inner_size, .. } => {
                        self.resize(**new_inner_size);
                    }

                    _ => (),
                },
                Event::RedrawRequested(window_id) if window_id == self.window.id() => {
                    self.update();
                    match self.render() {
                        Ok(_) => {}
                        // Reconfigure the surface if lost
                        Err(wgpu::SurfaceError::Lost | wgpu::SurfaceError::Outdated) => {
                            self.resize(self.size)
                        }
                        // The system is out of memory, we should probably quit
                        Err(wgpu::SurfaceError::OutOfMemory) => *control_flow = ControlFlow::Exit,
                        // All other errors should be resolved by the next frame
                        Err(e) => eprintln!("{:?}", e),
                    }
                }
                Event::MainEventsCleared => {
                    self.window.request_redraw();

                    update_function(&mut self, &input);

                    if self.exit_requested {
                        *control_flow = ControlFlow::Exit;
                    }
                }
                _ => (),
            }
        });
    }
}

/// Inits your frug instance and your event loop.
/// Returns a pair containing a FrugInstance and an EventLoop.
pub fn new(window_title: &str) -> (FrugInstance, EventLoop<()>) {
    let event_loop = EventLoop::new();
    let frug_instance = pollster::block_on(FrugInstance::new_instance(window_title, &event_loop));

    return (frug_instance, event_loop);
}

/// Creates a color.
/// Should receive in range from 0.0 - 1.0 the red, green, blue, and alpha channels.
/// * `red (f64)`   - The red channel.
/// * `green (f64)`   - The green channel.
/// * `blue (f64)`   - The blue channel.
/// * `alpha (f64)`   - The alpha channel.
///
/// # Example:
///
/// ```
/// frug::create_color(0.1, 0.2, 0.3, 1.0);
/// ```
pub fn create_color(red: f64, green: f64, blue: f64, alpha: f64) -> wgpu::Color {
    wgpu::Color {
        r: red,
        g: green,
        b: blue,
        a: alpha,
    }
}

// EOF
