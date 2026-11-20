use std::borrow::BorrowMut;

use smithay::backend::{
    egl::EGLContext,
    renderer::{
        gles::{GlesRenderer, GlesTexProgram, UniformName, UniformType},
        glow::GlowRenderer,
    },
};

const BLUR_FRAG: &str = include_str!("shaders/blur.frag");

pub struct BlurShader(GlesTexProgram);

impl BlurShader {
    pub fn init(renderer: &mut GlowRenderer) {
        let renderer: &mut GlesRenderer = renderer.borrow_mut();
        let program = renderer
            .compile_custom_texture_shader(
                BLUR_FRAG,
                &[
                    UniformName::new("size", UniformType::_2f),
                    UniformName::new("radius", UniformType::_1f),
                ],
            )
            .unwrap();
        renderer
            .egl_context()
            .user_data()
            .insert_if_missing(|| BlurShader(program));
    }
    pub fn get(ctx: &EGLContext) -> GlesTexProgram {
        ctx.user_data()
            .get::<BlurShader>()
            .expect("Blur Shader not initialized")
            .0
            .clone()
    }
}
