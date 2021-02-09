// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use winit::{event::{Event, WindowEvent}, event_loop::{ControlFlow, EventLoop}, window::Window};

pub fn begin() -> anyhow::Result<()> {
    let event_loop = EventLoop::new();
    let window = Window::new(&event_loop)?;

    futures::executor::block_on(run(event_loop, window))
}

async fn run(event_loop: EventLoop<()>, window: Window) -> anyhow::Result<()> {
    let instance = wgpu::Instance::new(wgpu::BackendBit::all());
    let surface = unsafe { instance.create_surface(&window) };

    let adapter = instance
        .request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::default(),
            // Request an adapter which can render to our surface
            compatible_surface: Some(&surface),
        })
        .await
        .ok_or_else(|| anyhow::anyhow!("failed to request adapter"))?;

    // Create the logical device and command queue
    let (device, queue) = adapter
        .request_device(
            &wgpu::DeviceDescriptor {
                features: wgpu::Features::empty(),
                limits: wgpu::Limits::default(),
                shader_validation: true,
            },
            None,
        )
        .await?;
    
    
    event_loop.run(move |event, _, control_flow| {
        match event {
            Event::WindowEvent {
                event: WindowEvent::CloseRequested,
                ..
            } => *control_flow = ControlFlow::Exit,
            _ => {}
        }
    })
}
