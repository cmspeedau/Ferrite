use std::borrow::BorrowMut;

use smithay::backend::{
    egl::EGLContext,
    renderer::{
        element::Kind,
        gles::{
            element::PixelShaderElement, GlesPixelProgram, GlesRenderer, Uniform, UniformName,
            UniformType,
        },
        glow::GlowRenderer,
    },
};
use smithay::utils::{Logical, Rectangle};

use crate::state::CONFIG;

const DOCK_FRAG: &str = include_str!("shaders/dock.frag");

pub struct DockShader(GlesPixelProgram);

impl DockShader {
    pub fn init(renderer: &mut GlowRenderer) {
        let renderer: &mut GlesRenderer = renderer.borrow_mut();
        let program = renderer
            .compile_custom_pixel_shader(
                DOCK_FRAG,
                &[
                    UniformName::new("radius", UniformType::_1f),
                    UniformName::new("slots", UniformType::_1f),
                ],
            )
            .unwrap();
        renderer
            .egl_context()
            .user_data()
            .insert_if_missing(|| DockShader(program));
    }

    pub fn get(ctx: &EGLContext) -> GlesPixelProgram {
        ctx.user_data()
            .get::<DockShader>()
            .expect("Dock Shader not initialized")
            .0
            .clone()
    }

    pub fn element(
        renderer: &mut GlowRenderer,
        geo: Rectangle<i32, Logical>,
    ) -> PixelShaderElement {
        PixelShaderElement::new(
            Self::get(renderer.egl_context()),
            geo,
            None,
            1.0,
            vec![
                Uniform::new("radius", 16.0f32),
                Uniform::new("slots", CONFIG.dock_apps.len() as f32),
            ],
            Kind::Unspecified,
        )
    }
}
