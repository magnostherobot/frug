//! FRUG is intended to provide a similar abstraction layer over graphics programming as to how SDL does for C++, meaning that it should provide developers enough control and flexibility to implement their own architectures & design patterns, yet simplifying the process of working with graphics so developers won't have to worry about implementing all the repetitive tasks related to getting things to the screen.
//! 
//! FRUG aims to include the following features (unchecked items are the ones still under development):
//! - [x] Window management
//! - [ ]  Loading & rendering textures
//! - [ ]  Rotating textures
//! - [ ]  Scaling textures
//! - [ ]  Alpha blending for textures
//! - [ ]  Choosing a specific backend (aka. Direct X, Metal, Vulkan, etc.)
//! - [ ]  Writing and using custom shaders
//! - [ ]  Handle window state events
//! - [ ]  Handle Mouse input
//! - [ ]  Handle Keyboard input
//! - [ ]  Playing audio
//! - [ ]  Configure audio


use winit::{
    event::{Event, WindowEvent},
    event_loop::{EventLoop, ControlFlow},
    window::Window
};

pub struct FrugInstance {
    surface: wgpu::Surface,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    size: winit::dpi::PhysicalSize<u32>,
    window: Window,
    background_color: wgpu::Color
}

impl FrugInstance {
    /// Creates a new instance of FrugInstance, instantiating the window, configuration, and the surface to draw in.
    pub async fn new_instance(window_title: &str, event_loop: &EventLoop<()>) -> Self {
        // Enable wgpu logging
        env_logger::init();

        // Setup
        let window = Window::new(&event_loop).unwrap();
        window.set_title(window_title);
        let size = window.inner_size();
        let background_color = wgpu::Color { r: 0.0, g: 0.0, b: 0.0, a: 1.0 };

        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::default());

        let surface = unsafe { 
            instance.create_surface(&window)
        }.unwrap();

        let adapter = instance.request_adapter(
            &wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::default(),
                compatible_surface: Some(&surface),
                force_fallback_adapter: false
            }
        ).await.expect("Failed to find an appropiate adapter.");

        let (device, queue) = adapter.request_device(
            &wgpu::DeviceDescriptor {
                label: None,
                features: wgpu::Features::empty(),
                limits: wgpu::Limits::default()
            }, None).await.expect("Failed to create device.");

        let surface_caps = surface.get_capabilities(&adapter);
        let surface_format = surface_caps.formats.iter()
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

        Self {
            window,
            surface,
            device,
            queue,
            config,
            size,
            background_color
        }
    }

    /// Resize the canvas for our window given a new defined size.
    pub fn resize(&mut self, new_size: winit::dpi::PhysicalSize<u32>) {
        if new_size.width > 0 && new_size.height > 0 {
            self.size = new_size;
            self.config.width = new_size.width;
            self.config.height = new_size.height;
            self.surface.configure(&self.device, &self.config);
        }
    }

    /// Render
    fn render(&mut self) -> Result<(), wgpu::SurfaceError> {
        let output = self.surface.get_current_texture()?;

        let view = output.texture.create_view(&wgpu::TextureViewDescriptor::default());

        let mut encoder = self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("Render Encoder")
        });

        {
            let _render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor { 
                label: Some("Render Pass"), 
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view, 
                    resolve_target: None, 
                    ops: wgpu::Operations { 
                        load: wgpu::LoadOp::Clear(self.background_color), 
                        store: true
                    }
                })], 
                depth_stencil_attachment: None
            });
        }

        self.queue.submit(std::iter::once(encoder.finish()));
        output.present();

        Ok(())
    }
}

/// Starts running your project.
/// 
/// Should receive a string which will be the title for the window created. It should also receive a loop which will be the main loop for your game/app.
/// * `window_title (&str)`         - The title for your window.
/// * `window_loop (static Fn())`   - The loop you want to execute with each frame.
/// 
/// # Example:
/// 
/// ```
/// let my_loop = || {
///     // your code
/// };
/// frug::run("My Game", my_loop);
/// ```
pub fn run<F: 'static + Fn()>(window_title: &str, window_loop: F) {
    // setup
    let event_loop = EventLoop::new();
    let mut frug_instance = pollster::block_on( FrugInstance::new_instance(window_title, &event_loop));

    // Run the loop
    event_loop.run(move |event, _, control_flow| {
        *control_flow = ControlFlow::Wait;

        // Act on events
        match event {
            Event::WindowEvent {
                ref event,
                window_id,
            } 
            // Window events
            if window_id == frug_instance.window.id() => match event {
                // Close
                WindowEvent::CloseRequested => {
                    *control_flow = ControlFlow::Exit;
                },

                // Resize
                WindowEvent::Resized(physical_size) => {
                    frug_instance.resize(*physical_size);
                },
                WindowEvent::ScaleFactorChanged { new_inner_size, .. } => {
                    frug_instance.resize(**new_inner_size);
                }
                _ => ()
            }
            Event::RedrawRequested(window_id) if window_id == frug_instance.window.id() => {
                // frug_instance.update();
                match frug_instance.render() {
                    Ok(_) => {}
                    // Reconfigure the surface if lost
                    Err(wgpu::SurfaceError::Lost) => frug_instance.resize(frug_instance.size),
                    // The system is out of memory, we should probably quit
                    Err(wgpu::SurfaceError::OutOfMemory) => *control_flow = ControlFlow::Exit,
                    // All other errors should be resolved by the next frame
                    Err(e) => eprintln!("{:?}", e),
                }
            }
            Event::MainEventsCleared => {
                frug_instance.window.request_redraw();
            }
            _ => (),
        }

        window_loop();
    });
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
    wgpu::Color { r: red, g: green, b: blue, a: alpha }
}

// EOF