//! Direct3D 11 + DirectComposition renderer for the transparent, click-through overlay.

use std::ffi::c_void;
use windows::core::{s, Interface, Result, PCSTR};
use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Direct3D::Fxc::*;
use windows::Win32::Graphics::Direct3D::*;
use windows::Win32::Graphics::Direct3D11::*;
use windows::Win32::Graphics::DirectComposition::*;
use windows::Win32::Graphics::Dxgi::Common::*;
use windows::Win32::Graphics::Dxgi::*;

use crate::cursor::CursorImage;

const SHADER_SRC: &str = include_str!("shaders.hlsl");

#[repr(C)]
#[derive(Clone, Copy, Default, Debug)]
pub struct Inst {
    pub center: [f32; 2],
    pub half: [f32; 2],
    pub color: [f32; 4],
    /// kind, rotation, param, additive
    pub p: [f32; 4],
    pub uv: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Default, Debug)]
pub struct RVert {
    pub pos: [f32; 2],
    pub v: f32,
    pub soft: f32,
    pub color: [f32; 4],
    pub additive: f32,
    pub pad: [f32; 3],
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct FrameCb {
    inv: [f32; 2],
    time: f32,
    pad0: f32,
    base: u32,
    pad1: [u32; 3],
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BlendMode {
    Over,
    Add,
}

#[derive(Clone, Copy, Debug)]
pub enum Cmd {
    Quads { blend: BlendMode, start: u32, count: u32 },
    Ribbon { start: u32, count: u32 },
}

#[derive(Default)]
pub struct FrameData {
    pub insts: Vec<Inst>,
    pub verts: Vec<RVert>,
    pub cmds: Vec<Cmd>,
}

impl FrameData {
    pub fn clear(&mut self) {
        self.insts.clear();
        self.verts.clear();
        self.cmds.clear();
    }

    /// Push a batch of quads; merges with the previous command when the blend mode matches.
    pub fn quads(&mut self, blend: BlendMode, items: impl IntoIterator<Item = Inst>) {
        let start = self.insts.len() as u32;
        self.insts.extend(items);
        let count = self.insts.len() as u32 - start;
        if count == 0 {
            return;
        }
        if let Some(Cmd::Quads { blend: b, start: s, count: c }) = self.cmds.last_mut() {
            if *b == blend && *s + *c == start {
                *c += count;
                return;
            }
        }
        self.cmds.push(Cmd::Quads { blend, start, count });
    }

    pub fn ribbon(&mut self, verts: &[RVert]) {
        if verts.is_empty() {
            return;
        }
        let start = self.verts.len() as u32;
        self.verts.extend_from_slice(verts);
        self.cmds.push(Cmd::Ribbon { start, count: verts.len() as u32 });
    }

    pub fn is_empty(&self) -> bool {
        self.cmds.is_empty()
    }
}

struct DynBuf {
    buf: ID3D11Buffer,
    srv: ID3D11ShaderResourceView,
    cap: u32,
}

pub struct Renderer {
    device: ID3D11Device,
    ctx: ID3D11DeviceContext,
    swapchain: IDXGISwapChain1,
    pub waitable: HANDLE,
    _dcomp: IDCompositionDevice,
    _target: IDCompositionTarget,
    _visual: IDCompositionVisual,
    rtv: Option<ID3D11RenderTargetView>,
    pub width: u32,
    pub height: u32,
    vs_quad: ID3D11VertexShader,
    ps_quad: ID3D11PixelShader,
    vs_rib: ID3D11VertexShader,
    ps_rib: ID3D11PixelShader,
    cb: ID3D11Buffer,
    blend_over: ID3D11BlendState,
    blend_add: ID3D11BlendState,
    raster: ID3D11RasterizerState,
    sampler: ID3D11SamplerState,
    inst: Option<DynBuf>,
    rib: Option<DynBuf>,
    cursor_srv: Option<ID3D11ShaderResourceView>,
    cursor_key: (isize, bool),
}

fn compile(entry: PCSTR, target: PCSTR) -> Result<Vec<u8>> {
    unsafe {
        let mut code: Option<ID3DBlob> = None;
        let mut err: Option<ID3DBlob> = None;
        let r = D3DCompile(
            SHADER_SRC.as_ptr() as *const c_void,
            SHADER_SRC.len(),
            s!("shaders.hlsl"),
            None,
            None,
            entry,
            target,
            D3DCOMPILE_OPTIMIZATION_LEVEL3,
            0,
            &mut code,
            Some(&mut err),
        );
        if let Err(e) = r {
            if let Some(err) = err {
                let msg = std::slice::from_raw_parts(err.GetBufferPointer() as *const u8, err.GetBufferSize());
                crate::log(&format!("shader error: {}", String::from_utf8_lossy(msg)));
            }
            return Err(e);
        }
        let code = code.unwrap();
        Ok(std::slice::from_raw_parts(code.GetBufferPointer() as *const u8, code.GetBufferSize()).to_vec())
    }
}

fn blend_state(device: &ID3D11Device, src: D3D11_BLEND, dst: D3D11_BLEND) -> Result<ID3D11BlendState> {
    let rt = D3D11_RENDER_TARGET_BLEND_DESC {
        BlendEnable: true.into(),
        SrcBlend: src,
        DestBlend: dst,
        BlendOp: D3D11_BLEND_OP_ADD,
        SrcBlendAlpha: src,
        DestBlendAlpha: dst,
        BlendOpAlpha: D3D11_BLEND_OP_ADD,
        RenderTargetWriteMask: D3D11_COLOR_WRITE_ENABLE_ALL.0 as u8,
    };
    let desc = D3D11_BLEND_DESC { AlphaToCoverageEnable: false.into(), IndependentBlendEnable: false.into(), RenderTarget: [rt; 8] };
    let mut out = None;
    unsafe { device.CreateBlendState(&desc, Some(&mut out))? };
    Ok(out.unwrap())
}

impl Renderer {
    pub fn new(hwnd: HWND, width: u32, height: u32) -> Result<Self> {
        unsafe {
            let mut device = None;
            let mut ctx = None;
            let levels = [D3D_FEATURE_LEVEL_11_1, D3D_FEATURE_LEVEL_11_0, D3D_FEATURE_LEVEL_10_1, D3D_FEATURE_LEVEL_10_0];
            D3D11CreateDevice(
                None,
                D3D_DRIVER_TYPE_HARDWARE,
                HMODULE::default(),
                D3D11_CREATE_DEVICE_BGRA_SUPPORT,
                Some(&levels),
                D3D11_SDK_VERSION,
                Some(&mut device),
                None,
                Some(&mut ctx),
            )?;
            let device: ID3D11Device = device.unwrap();
            let ctx: ID3D11DeviceContext = ctx.unwrap();

            let dxgi_device: IDXGIDevice = device.cast()?;
            let adapter = dxgi_device.GetAdapter()?;
            let factory: IDXGIFactory2 = adapter.GetParent()?;

            let desc = DXGI_SWAP_CHAIN_DESC1 {
                Width: width.max(1),
                Height: height.max(1),
                Format: DXGI_FORMAT_B8G8R8A8_UNORM,
                Stereo: false.into(),
                SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
                BufferUsage: DXGI_USAGE_RENDER_TARGET_OUTPUT,
                BufferCount: 2,
                Scaling: DXGI_SCALING_STRETCH,
                SwapEffect: DXGI_SWAP_EFFECT_FLIP_SEQUENTIAL,
                AlphaMode: DXGI_ALPHA_MODE_PREMULTIPLIED,
                Flags: DXGI_SWAP_CHAIN_FLAG_FRAME_LATENCY_WAITABLE_OBJECT.0 as u32,
            };
            let swapchain = factory.CreateSwapChainForComposition(&device, &desc, None)?;
            let sc2: IDXGISwapChain2 = swapchain.cast()?;
            sc2.SetMaximumFrameLatency(1)?;
            let waitable = sc2.GetFrameLatencyWaitableObject();

            let dcomp: IDCompositionDevice = DCompositionCreateDevice(&dxgi_device)?;
            let target = dcomp.CreateTargetForHwnd(hwnd, true)?;
            let visual = dcomp.CreateVisual()?;
            visual.SetContent(&swapchain)?;
            target.SetRoot(&visual)?;
            dcomp.Commit()?;

            let mut vs_quad = None;
            let mut ps_quad = None;
            let mut vs_rib = None;
            let mut ps_rib = None;
            device.CreateVertexShader(&compile(s!("VSQuad"), s!("vs_5_0"))?, None, Some(&mut vs_quad))?;
            device.CreatePixelShader(&compile(s!("PSQuad"), s!("ps_5_0"))?, None, Some(&mut ps_quad))?;
            device.CreateVertexShader(&compile(s!("VSRibbon"), s!("vs_5_0"))?, None, Some(&mut vs_rib))?;
            device.CreatePixelShader(&compile(s!("PSRibbon"), s!("ps_5_0"))?, None, Some(&mut ps_rib))?;

            let mut cb = None;
            device.CreateBuffer(
                &D3D11_BUFFER_DESC {
                    ByteWidth: std::mem::size_of::<FrameCb>() as u32,
                    Usage: D3D11_USAGE_DYNAMIC,
                    BindFlags: D3D11_BIND_CONSTANT_BUFFER.0 as u32,
                    CPUAccessFlags: D3D11_CPU_ACCESS_WRITE.0 as u32,
                    ..Default::default()
                },
                None,
                Some(&mut cb),
            )?;

            let blend_over = blend_state(&device, D3D11_BLEND_ONE, D3D11_BLEND_INV_SRC_ALPHA)?;
            let blend_add = blend_state(&device, D3D11_BLEND_ONE, D3D11_BLEND_ONE)?;

            let mut raster = None;
            device.CreateRasterizerState(
                &D3D11_RASTERIZER_DESC {
                    FillMode: D3D11_FILL_SOLID,
                    CullMode: D3D11_CULL_NONE,
                    DepthClipEnable: true.into(),
                    ..Default::default()
                },
                Some(&mut raster),
            )?;

            let mut sampler = None;
            device.CreateSamplerState(
                &D3D11_SAMPLER_DESC {
                    Filter: D3D11_FILTER_MIN_MAG_MIP_LINEAR,
                    AddressU: D3D11_TEXTURE_ADDRESS_BORDER,
                    AddressV: D3D11_TEXTURE_ADDRESS_BORDER,
                    AddressW: D3D11_TEXTURE_ADDRESS_BORDER,
                    BorderColor: [0.0; 4],
                    MaxLOD: f32::MAX,
                    ..Default::default()
                },
                Some(&mut sampler),
            )?;

            let mut r = Self {
                device,
                ctx,
                swapchain,
                waitable,
                _dcomp: dcomp,
                _target: target,
                _visual: visual,
                rtv: None,
                width: width.max(1),
                height: height.max(1),
                vs_quad: vs_quad.unwrap(),
                ps_quad: ps_quad.unwrap(),
                vs_rib: vs_rib.unwrap(),
                ps_rib: ps_rib.unwrap(),
                cb: cb.unwrap(),
                blend_over,
                blend_add,
                raster: raster.unwrap(),
                sampler: sampler.unwrap(),
                inst: None,
                rib: None,
                cursor_srv: None,
                cursor_key: (0, false),
            };
            r.create_rtv()?;
            Ok(r)
        }
    }

    fn create_rtv(&mut self) -> Result<()> {
        unsafe {
            let tex: ID3D11Texture2D = self.swapchain.GetBuffer(0)?;
            let mut rtv = None;
            self.device.CreateRenderTargetView(&tex, None, Some(&mut rtv))?;
            self.rtv = rtv;
        }
        Ok(())
    }

    pub fn resize(&mut self, width: u32, height: u32) -> Result<()> {
        let (width, height) = (width.max(1), height.max(1));
        if width == self.width && height == self.height {
            return Ok(());
        }
        unsafe {
            self.ctx.OMSetRenderTargets(None, None);
            self.rtv = None;
            self.ctx.Flush();
            self.swapchain.ResizeBuffers(
                0,
                width,
                height,
                DXGI_FORMAT_UNKNOWN,
                DXGI_SWAP_CHAIN_FLAG(DXGI_SWAP_CHAIN_FLAG_FRAME_LATENCY_WAITABLE_OBJECT.0),
            )?;
        }
        self.width = width;
        self.height = height;
        self.create_rtv()
    }

    pub fn set_cursor(&mut self, key: (isize, bool), img: &CursorImage) {
        if self.cursor_key == key && self.cursor_srv.is_some() {
            return;
        }
        unsafe {
            let desc = D3D11_TEXTURE2D_DESC {
                Width: img.w,
                Height: img.h,
                MipLevels: 1,
                ArraySize: 1,
                Format: DXGI_FORMAT_B8G8R8A8_UNORM,
                SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
                Usage: D3D11_USAGE_IMMUTABLE,
                BindFlags: D3D11_BIND_SHADER_RESOURCE.0 as u32,
                CPUAccessFlags: 0,
                MiscFlags: 0,
            };
            let data = D3D11_SUBRESOURCE_DATA { pSysMem: img.pixels.as_ptr() as *const c_void, SysMemPitch: img.w * 4, SysMemSlicePitch: 0 };
            let mut tex = None;
            if self.device.CreateTexture2D(&desc, Some(&data), Some(&mut tex)).is_ok() {
                let mut srv = None;
                if self.device.CreateShaderResourceView(&tex.unwrap(), None, Some(&mut srv)).is_ok() {
                    self.cursor_srv = srv;
                    self.cursor_key = key;
                }
            }
        }
    }

    fn ensure_buf(device: &ID3D11Device, slot: &mut Option<DynBuf>, needed: u32, stride: u32) -> Result<()> {
        if let Some(b) = slot {
            if b.cap >= needed {
                return Ok(());
            }
        }
        let cap = needed.next_power_of_two().max(256);
        unsafe {
            let mut buf = None;
            device.CreateBuffer(
                &D3D11_BUFFER_DESC {
                    ByteWidth: cap * stride,
                    Usage: D3D11_USAGE_DYNAMIC,
                    BindFlags: D3D11_BIND_SHADER_RESOURCE.0 as u32,
                    CPUAccessFlags: D3D11_CPU_ACCESS_WRITE.0 as u32,
                    MiscFlags: D3D11_RESOURCE_MISC_BUFFER_STRUCTURED.0 as u32,
                    StructureByteStride: stride,
                },
                None,
                Some(&mut buf),
            )?;
            let buf = buf.unwrap();
            let srv_desc = D3D11_SHADER_RESOURCE_VIEW_DESC {
                Format: DXGI_FORMAT_UNKNOWN,
                ViewDimension: D3D_SRV_DIMENSION_BUFFER,
                Anonymous: D3D11_SHADER_RESOURCE_VIEW_DESC_0 {
                    Buffer: D3D11_BUFFER_SRV {
                        Anonymous1: D3D11_BUFFER_SRV_0 { FirstElement: 0 },
                        Anonymous2: D3D11_BUFFER_SRV_1 { NumElements: cap },
                    },
                },
            };
            let mut srv = None;
            device.CreateShaderResourceView(&buf, Some(&srv_desc), Some(&mut srv))?;
            *slot = Some(DynBuf { buf, srv: srv.unwrap(), cap });
        }
        Ok(())
    }

    fn upload<T: Copy>(ctx: &ID3D11DeviceContext, buf: &ID3D11Buffer, data: &[T]) -> Result<()> {
        unsafe {
            let mut m = D3D11_MAPPED_SUBRESOURCE::default();
            ctx.Map(buf, 0, D3D11_MAP_WRITE_DISCARD, 0, Some(&mut m))?;
            std::ptr::copy_nonoverlapping(data.as_ptr(), m.pData as *mut T, data.len());
            ctx.Unmap(buf, 0);
        }
        Ok(())
    }

    fn set_cb(&self, time: f32, base: u32) -> Result<()> {
        let cb = FrameCb { inv: [1.0 / self.width as f32, 1.0 / self.height as f32], time, pad0: 0.0, base, pad1: [0; 3] };
        Self::upload(&self.ctx, &self.cb, std::slice::from_ref(&cb))
    }

    /// Renders the frame and presents it. Returns Err on device loss.
    pub fn render(&mut self, frame: &FrameData, time: f32, sync_interval: u32) -> Result<()> {
        unsafe {
            let rtv = self.rtv.clone().unwrap();
            self.ctx.OMSetRenderTargets(Some(&[Some(rtv.clone())]), None);
            self.ctx.ClearRenderTargetView(&rtv, &[0.0, 0.0, 0.0, 0.0]);

            if !frame.is_empty() {
                Self::ensure_buf(&self.device, &mut self.inst, frame.insts.len() as u32, std::mem::size_of::<Inst>() as u32)?;
                Self::ensure_buf(&self.device, &mut self.rib, frame.verts.len() as u32, std::mem::size_of::<RVert>() as u32)?;
                let inst = self.inst.as_ref().unwrap();
                let rib = self.rib.as_ref().unwrap();
                if !frame.insts.is_empty() {
                    Self::upload(&self.ctx, &inst.buf, &frame.insts)?;
                }
                if !frame.verts.is_empty() {
                    Self::upload(&self.ctx, &rib.buf, &frame.verts)?;
                }

                let vp = D3D11_VIEWPORT { TopLeftX: 0.0, TopLeftY: 0.0, Width: self.width as f32, Height: self.height as f32, MinDepth: 0.0, MaxDepth: 1.0 };
                self.ctx.RSSetViewports(Some(&[vp]));
                self.ctx.RSSetState(&self.raster);
                self.ctx.IASetInputLayout(None);
                self.ctx.VSSetConstantBuffers(0, Some(&[Some(self.cb.clone())]));
                self.ctx.PSSetConstantBuffers(0, Some(&[Some(self.cb.clone())]));
                self.ctx.VSSetShaderResources(0, Some(&[Some(inst.srv.clone()), None, Some(rib.srv.clone())]));
                self.ctx.PSSetShaderResources(1, Some(&[self.cursor_srv.clone()]));
                self.ctx.PSSetSamplers(0, Some(&[Some(self.sampler.clone())]));

                for cmd in &frame.cmds {
                    match *cmd {
                        Cmd::Quads { blend, start, count } => {
                            self.set_cb(time, start)?;
                            let bs = if blend == BlendMode::Add { &self.blend_add } else { &self.blend_over };
                            self.ctx.OMSetBlendState(bs, Some(&[0.0; 4]), 0xffff_ffff);
                            self.ctx.IASetPrimitiveTopology(D3D_PRIMITIVE_TOPOLOGY_TRIANGLESTRIP);
                            self.ctx.VSSetShader(&self.vs_quad, None);
                            self.ctx.PSSetShader(&self.ps_quad, None);
                            self.ctx.DrawInstanced(4, count, 0, 0);
                        }
                        Cmd::Ribbon { start, count } => {
                            self.set_cb(time, start)?;
                            self.ctx.OMSetBlendState(&self.blend_over, Some(&[0.0; 4]), 0xffff_ffff);
                            self.ctx.IASetPrimitiveTopology(D3D_PRIMITIVE_TOPOLOGY_TRIANGLELIST);
                            self.ctx.VSSetShader(&self.vs_rib, None);
                            self.ctx.PSSetShader(&self.ps_rib, None);
                            self.ctx.Draw(count, 0);
                        }
                    }
                }
            }

            self.swapchain.Present(sync_interval.clamp(1, 4), DXGI_PRESENT(0)).ok()?;
        }
        Ok(())
    }

}

impl Drop for Renderer {
    fn drop(&mut self) {
        unsafe {
            if !self.waitable.is_invalid() {
                let _ = CloseHandle(self.waitable);
            }
        }
    }
}
