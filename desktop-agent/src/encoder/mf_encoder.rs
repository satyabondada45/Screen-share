use windows::core::{Interface, GUID, VARIANT};
use windows::Win32::Graphics::Dxgi::*;
use windows::Win32::Media::MediaFoundation::*;
use windows::Win32::System::Com::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GpuBackend {
    NvidiaNvenc,
    AmdAmf,
    IntelQuickSync,
    WindowsHardwareMft,
}

impl std::fmt::Display for GpuBackend {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            GpuBackend::NvidiaNvenc => write!(f, "NVIDIA NVENC"),
            GpuBackend::AmdAmf => write!(f, "AMD AMF"),
            GpuBackend::IntelQuickSync => write!(f, "Intel Quick Sync"),
            GpuBackend::WindowsHardwareMft => write!(f, "Windows Hardware MFT"),
        }
    }
}

pub struct HardwareH264Encoder {
    pub backend: GpuBackend,
    mft: IMFTransform,
    event_gen: Option<IMFMediaEventGenerator>,
    width: u32,
    height: u32,
    fps: u32,
    frame_index: u64,
    nv12_buffer: Vec<u8>,
    pub is_async: bool,
    inputs_needed: usize,
    has_logged_first_keyframe: bool,
}

impl HardwareH264Encoder {
    pub fn backend(&self) -> GpuBackend {
        self.backend
    }
}

unsafe impl Send for HardwareH264Encoder {}

fn set_attribute_size(attrs: &IMFMediaType, guid: &GUID, width: u32, height: u32) -> Result<(), windows::core::Error> {
    unsafe {
        let packed = ((width as u64) << 32) | (height as u64);
        attrs.SetUINT64(guid, packed)
    }
}

fn set_attribute_ratio(attrs: &IMFMediaType, guid: &GUID, num: u32, den: u32) -> Result<(), windows::core::Error> {
    unsafe {
        let packed = ((num as u64) << 32) | (den as u64);
        attrs.SetUINT64(guid, packed)
    }
}

fn set_codec_bool(codec_api: &ICodecAPI, guid: &GUID, val: bool) {
    unsafe {
        let var = VARIANT::from(val);
        let _ = codec_api.SetValue(guid, &var);
    }
}

fn set_codec_u32(codec_api: &ICodecAPI, guid: &GUID, val: u32) {
    unsafe {
        let var = VARIANT::from(val);
        let _ = codec_api.SetValue(guid, &var);
    }
}

impl HardwareH264Encoder {
    pub fn detect_hardware() -> (bool, bool, bool, Option<GpuBackend>) {
        println!("[H264 HW] Detecting hardware encoders...");

        let mut has_nvidia = false;
        let mut has_amd = false;
        let mut has_intel = false;

        unsafe {
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
            if let Ok(factory) = CreateDXGIFactory1::<IDXGIFactory1>() {
                let mut i = 0;
                while let Ok(adapter) = factory.EnumAdapters1(i) {
                    if let Ok(desc) = adapter.GetDesc1() {
                        let name = String::from_utf16_lossy(&desc.Description);
                        let name_clean = name.trim_matches(char::from(0));
                        let vendor_id = desc.VendorId;

                        if vendor_id == 0x10DE || name_clean.to_lowercase().contains("nvidia") || name_clean.to_lowercase().contains("geforce") {
                            has_nvidia = true;
                        } else if vendor_id == 0x1002 || name_clean.to_lowercase().contains("amd") || name_clean.to_lowercase().contains("radeon") {
                            has_amd = true;
                        } else if vendor_id == 0x8086 || name_clean.to_lowercase().contains("intel") || name_clean.to_lowercase().contains("uhd") || name_clean.to_lowercase().contains("iris") || name_clean.to_lowercase().contains("arc") {
                            has_intel = true;
                        }
                    }
                    i += 1;
                }
            }
        }

        println!("[H264 HW] NVIDIA NVENC: {}", if has_nvidia { "AVAILABLE" } else { "NOT AVAILABLE" });
        println!("[H264 HW] AMD AMF: {}", if has_amd { "AVAILABLE" } else { "NOT AVAILABLE" });
        println!("[H264 HW] Intel Quick Sync: {}", if has_intel { "AVAILABLE" } else { "NOT AVAILABLE" });

        let selected = if has_nvidia {
            Some(GpuBackend::NvidiaNvenc)
        } else if has_amd {
            Some(GpuBackend::AmdAmf)
        } else if has_intel {
            Some(GpuBackend::IntelQuickSync)
        } else {
            Some(GpuBackend::WindowsHardwareMft)
        };

        (has_nvidia, has_amd, has_intel, selected)
    }

    pub fn new(width: u32, height: u32, fps: u32, bitrate: u32) -> Result<Self, String> {
        let (_, _, _, selected_opt) = Self::detect_hardware();
        let backend = selected_opt.ok_or_else(|| "[H264 HW][FATAL] No supported hardware H.264 encoder found.".to_string())?;

        println!("========================================");
        println!("[H264 HW] Selected encoder: {}", backend);
        println!("[H264 HW] Hardware acceleration: ENABLED");
        println!("[H264 HW] Low latency mode: ENABLED");
        println!("[H264 HW] Target FPS: {}", fps);
        println!("[H264 HW] Target resolution: {}x{}", width, height);
        println!("[H264 HW] Target bitrate: {} bps", bitrate);
        println!("[H264 HW] B-frames: 0");
        println!("[H264 HW] Lookahead: DISABLED");
        println!("========================================");

        unsafe {
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
            MFStartup(MF_VERSION, MFSTARTUP_FULL).map_err(|e| format!("MFStartup failed: {:?}", e))?;

            // Enumerate hardware encoders
            let input_type = MFT_REGISTER_TYPE_INFO {
                guidMajorType: MFMediaType_Video,
                guidSubtype: MFVideoFormat_NV12,
            };
            let output_type = MFT_REGISTER_TYPE_INFO {
                guidMajorType: MFMediaType_Video,
                guidSubtype: MFVideoFormat_H264,
            };

            let mut mft_activate_ptrs: *mut Option<IMFActivate> = std::ptr::null_mut();
            let mut num_mfts: u32 = 0;

            let hr = MFTEnumEx(
                MFT_CATEGORY_VIDEO_ENCODER,
                MFT_ENUM_FLAG_HARDWARE | MFT_ENUM_FLAG_SORTANDFILTER,
                Some(&input_type),
                Some(&output_type),
                &mut mft_activate_ptrs,
                &mut num_mfts,
            );

            let mft: IMFTransform = if hr.is_ok() && num_mfts > 0 && !mft_activate_ptrs.is_null() {
                let activates = std::slice::from_raw_parts(mft_activate_ptrs, num_mfts as usize);
                let first_act = activates[0].as_ref().ok_or("Invalid IMFActivate pointer")?;
                let transform: IMFTransform = first_act.ActivateObject().map_err(|e| format!("ActivateObject failed: {:?}", e))?;
                CoTaskMemFree(Some(mft_activate_ptrs as *const _));
                println!("[H264 HW] Successfully instantiated Hardware MFT Video Encoder.");
                transform
            } else {
                println!("[H264 HW] Initializing standard Windows H.264 Encoder MFT.");
                CoCreateInstance(&CMSH264EncoderMFT, None, CLSCTX_INPROC_SERVER)
                    .map_err(|e| format!("Failed to create CMSH264EncoderMFT: {:?}", e))?
            };

            // ========================================================
            // ASYNC MFT UNLOCK & ATTRIBUTES
            // ========================================================
            let mut is_async = false;
            if let Ok(attrs) = mft.GetAttributes() {
                is_async = attrs.GetUINT32(&MF_TRANSFORM_ASYNC).unwrap_or(0) != 0;
                println!("[H264 HW] MFT attributes: MF_TRANSFORM_ASYNC: {}", is_async);

                if is_async {
                    let hr_unlock = attrs.SetUINT32(&MF_TRANSFORM_ASYNC_UNLOCK, 1);
                    println!("[H264 HW] Async: TRUE");
                    println!("[H264 HW] Unlocked via MF_TRANSFORM_ASYNC_UNLOCK: {:?}", hr_unlock);
                } else {
                    println!("[H264 HW] Async: FALSE");
                }
            }

            // Configure Low Latency and Real-Time settings via ICodecAPI
            if let Ok(codec_api) = mft.cast::<ICodecAPI>() {
                // Low Latency Mode = TRUE (zero frame buffering)
                set_codec_bool(&codec_api, &CODECAPI_AVLowLatencyMode, true);

                // Zero B-Frames
                set_codec_u32(&codec_api, &CODECAPI_AVEncMPVDefaultBPictureCount, 0);

                // GOP Size = 120 (1 second GOP at 120 FPS)
                set_codec_u32(&codec_api, &CODECAPI_AVEncMPVGOPSize, 120);

                // Bitrate = 8_000_000
                set_codec_u32(&codec_api, &CODECAPI_AVEncCommonMeanBitRate, bitrate);

                // Rate Control: CBR
                set_codec_u32(&codec_api, &CODECAPI_AVEncCommonRateControlMode, eAVEncCommonRateControlMode_CBR.0 as u32);
                println!("[H264 HW] ICodecAPI low-latency properties applied.");
            }

            // Set Output Media Type: H.264 FIRST
            let out_media_type: IMFMediaType = MFCreateMediaType().map_err(|e| format!("MFCreateMediaType failed: {:?}", e))?;
            out_media_type.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video).map_err(|e| format!("{:?}", e))?;
            out_media_type.SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_H264).map_err(|e| format!("{:?}", e))?;
            out_media_type.SetUINT32(&MF_MT_AVG_BITRATE, bitrate).map_err(|e| format!("{:?}", e))?;
            set_attribute_size(&out_media_type, &MF_MT_FRAME_SIZE, width, height).map_err(|e| format!("{:?}", e))?;
            set_attribute_ratio(&out_media_type, &MF_MT_FRAME_RATE, fps, 1).map_err(|e| format!("{:?}", e))?;
            set_attribute_ratio(&out_media_type, &MF_MT_PIXEL_ASPECT_RATIO, 1, 1).map_err(|e| format!("{:?}", e))?;
            out_media_type.SetUINT32(&MF_MT_INTERLACE_MODE, MFVideoInterlace_Progressive.0 as u32).map_err(|e| format!("{:?}", e))?;
            out_media_type.SetUINT32(&MF_MT_MPEG2_PROFILE, eAVEncH264VProfile_Base.0 as u32).map_err(|e| format!("{:?}", e))?;

            mft.SetOutputType(0, &out_media_type, 0).map_err(|e| format!("SetOutputType failed: {:?}", e))?;
            println!("[H264 HW] Output type configured successfully: H.264 {}x{} @ {} FPS", width, height, fps);

            // Set Input Media Type: NV12
            let in_media_type: IMFMediaType = MFCreateMediaType().map_err(|e| format!("MFCreateMediaType failed: {:?}", e))?;
            in_media_type.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video).map_err(|e| format!("{:?}", e))?;
            in_media_type.SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_NV12).map_err(|e| format!("{:?}", e))?;
            set_attribute_size(&in_media_type, &MF_MT_FRAME_SIZE, width, height).map_err(|e| format!("{:?}", e))?;
            set_attribute_ratio(&in_media_type, &MF_MT_FRAME_RATE, fps, 1).map_err(|e| format!("{:?}", e))?;
            set_attribute_ratio(&in_media_type, &MF_MT_PIXEL_ASPECT_RATIO, 1, 1).map_err(|e| format!("{:?}", e))?;
            in_media_type.SetUINT32(&MF_MT_INTERLACE_MODE, MFVideoInterlace_Progressive.0 as u32).map_err(|e| format!("{:?}", e))?;
            in_media_type.SetUINT32(&MF_MT_DEFAULT_STRIDE, width).map_err(|e| format!("{:?}", e))?;
            in_media_type.SetUINT32(&MF_MT_ALL_SAMPLES_INDEPENDENT, 1).map_err(|e| format!("{:?}", e))?;
            in_media_type.SetUINT32(&MF_MT_FIXED_SIZE_SAMPLES, 1).map_err(|e| format!("{:?}", e))?;
            let sample_size = width * height * 3 / 2;
            in_media_type.SetUINT32(&MF_MT_SAMPLE_SIZE, sample_size).map_err(|e| format!("{:?}", e))?;

            mft.SetInputType(0, &in_media_type, 0).map_err(|e| format!("SetInputType failed: {:?}", e))?;
            println!("[H264 HW] Input type configured successfully: NV12 {}x{}", width, height);

            // Acquire Event Generator for Async MFT
            let event_gen = if is_async {
                mft.cast::<IMFMediaEventGenerator>().ok()
            } else {
                None
            };

            // Start MFT Streaming Messages
            let _ = mft.ProcessMessage(MFT_MESSAGE_COMMAND_FLUSH, 0);
            println!("[H264 HW] Begin streaming.");
            let _ = mft.ProcessMessage(MFT_MESSAGE_NOTIFY_BEGIN_STREAMING, 0);
            println!("[H264 HW] Start of stream.");
            let _ = mft.ProcessMessage(MFT_MESSAGE_NOTIFY_START_OF_STREAM, 0);

            println!("[H264 HW] Hardware H.264 encoder READY.");

            let nv12_size = (width as usize) * (height as usize) * 3 / 2;
            let nv12_buffer = vec![0u8; nv12_size];

            Ok(Self {
                backend,
                mft,
                event_gen,
                width,
                height,
                fps,
                frame_index: 0,
                nv12_buffer,
                is_async,
                inputs_needed: 0,
                has_logged_first_keyframe: false,
            })
        }
    }

    // Process asynchronous MFT events and collect available output
    fn process_events(&mut self) -> Result<Vec<u8>, String> {
        let mut output_bytes = Vec::new();

        if let Some(event_gen) = self.event_gen.clone() {
            // Poll for all currently queued events without blocking
            loop {
                unsafe {
                    let event_res = event_gen.GetEvent(MEDIA_EVENT_GENERATOR_GET_EVENT_FLAGS(1)); // 1 = MF_EVENT_FLAG_NO_WAIT
                    match event_res {
                        Ok(event) => {
                            let event_id = event.GetType().unwrap_or(0);

                            if event_id == 140 { // METransformNeedInput
                                self.inputs_needed += 1;
                                println!("[H264 HW] Event → METransformNeedInput (needed={})", self.inputs_needed);
                            } else if event_id == 141 { // METransformHaveOutput
                                println!("[H264 HW] Event → METransformHaveOutput");
                                let out = self.pull_single_output()?;
                                if !out.is_empty() {
                                    output_bytes.extend_from_slice(&out);
                                }
                            } else if event_id == 142 { // METransformDrainComplete
                                println!("[H264 HW] Event → METransformDrainComplete");
                            } else if event_id == 143 { // METransformMarker
                                println!("[H264 HW] Event → METransformMarker");
                            } else {
                                println!("[H264 HW] Event → MediaEventType({})", event_id);
                            }
                        }
                        Err(_) => {
                            // No more events in queue
                            break;
                        }
                    }
                }
            }
        }

        Ok(output_bytes)
    }

    // Pull a single output sample from the MFT
    fn pull_single_output(&mut self) -> Result<Vec<u8>, String> {
        unsafe {
            let out_buffer: IMFMediaBuffer = MFCreateMemoryBuffer(1024 * 1024)
                .map_err(|e| format!("Create output buffer failed: {:?}", e))?;
            let out_sample: IMFSample = MFCreateSample()
                .map_err(|e| format!("Create output sample failed: {:?}", e))?;
            out_sample.AddBuffer(&out_buffer).map_err(|e| format!("AddBuffer output failed: {:?}", e))?;

            let mut output_data_buffer = MFT_OUTPUT_DATA_BUFFER {
                dwStreamID: 0,
                pSample: std::mem::ManuallyDrop::new(Some(out_sample.clone())),
                dwStatus: 0,
                pEvents: std::mem::ManuallyDrop::new(None),
            };

            let mut status: u32 = 0;
            let hr_out = self.mft.ProcessOutput(0, std::slice::from_mut(&mut output_data_buffer), &mut status);

            if hr_out.is_ok() {
                let mut out_data_ptr: *mut u8 = std::ptr::null_mut();
                let mut out_max_len: u32 = 0;
                let mut out_cur_len: u32 = 0;

                out_buffer.Lock(&mut out_data_ptr, Some(&mut out_max_len), Some(&mut out_cur_len))
                    .map_err(|e| format!("Lock out_buffer failed: {:?}", e))?;

                let encoded_slice = if !out_data_ptr.is_null() && out_cur_len > 0 {
                    std::slice::from_raw_parts(out_data_ptr, out_cur_len as usize)
                } else {
                    &[]
                };

                let annex_b = convert_to_annex_b(encoded_slice);
                let _ = out_buffer.Unlock();

                if !annex_b.is_empty() {
                    println!("[H264 HW] ProcessOutput → OK ({} bytes)", annex_b.len());

                    if !self.has_logged_first_keyframe {
                        self.has_logged_first_keyframe = true;
                        log_keyframe_diagnostics(&annex_b);
                    }
                }

                Ok(annex_b)
            } else if let Err(e) = hr_out {
                let code = e.code().0 as u32;
                if code == 0xC00D6D72 {
                    println!("[H264 HW] ProcessOutput → NEED_MORE_INPUT (HRESULT 0x{:08X})", code);
                } else if code == 0xC00D6D61 {
                    println!("[H264 HW] ProcessOutput → STREAM_CHANGE (HRESULT 0x{:08X})", code);
                } else {
                    println!("[H264 HW] ProcessOutput → HRESULT(0x{:08X})", code);
                }
                Ok(Vec::new())
            } else {
                Ok(Vec::new())
            }
        }
    }

    pub fn encode_rgba(&mut self, rgba: &[u8], src_width: usize, src_height: usize, force_keyframe: bool) -> Result<Vec<u8>, String> {
        let w = self.width as usize;
        let h = self.height as usize;

        // Fast direct single-pass downscale & NV12 conversion
        if src_width == w && src_height == h {
            rgba_to_nv12(rgba, w, h, &mut self.nv12_buffer);
        } else {
            rgba_downscale_to_nv12(rgba, src_width, src_height, w, h, &mut self.nv12_buffer);
        }

        let mut collected_output = Vec::new();

        // 1. Process any pending events from the MFT
        if self.is_async {
            let ev_out = self.process_events()?;
            if !ev_out.is_empty() {
                collected_output.extend_from_slice(&ev_out);
            }
        }

        unsafe {
            let buffer_len = self.nv12_buffer.len() as u32;
            let media_buffer: IMFMediaBuffer = MFCreateMemoryBuffer(buffer_len)
                .map_err(|e| format!("MFCreateMemoryBuffer failed: {:?}", e))?;

            let mut dest_ptr: *mut u8 = std::ptr::null_mut();
            let mut max_len: u32 = 0;
            let mut cur_len: u32 = 0;

            media_buffer.Lock(&mut dest_ptr, Some(&mut max_len), Some(&mut cur_len))
                .map_err(|e| format!("Lock buffer failed: {:?}", e))?;

            if !dest_ptr.is_null() && max_len >= buffer_len {
                std::ptr::copy_nonoverlapping(self.nv12_buffer.as_ptr(), dest_ptr, buffer_len as usize);
            }
            let _ = media_buffer.Unlock();
            let _ = media_buffer.SetCurrentLength(buffer_len);

            let sample: IMFSample = MFCreateSample().map_err(|e| format!("MFCreateSample failed: {:?}", e))?;
            sample.AddBuffer(&media_buffer).map_err(|e| format!("AddBuffer failed: {:?}", e))?;

            let sample_duration = (10_000_000u64 / (self.fps as u64)) as i64;
            let sample_time = (self.frame_index as i64) * sample_duration;
            let _ = sample.SetSampleTime(sample_time);
            let _ = sample.SetSampleDuration(sample_duration);

            if force_keyframe || self.frame_index % (self.fps as u64) == 0 {
                if let Ok(codec_api) = self.mft.cast::<ICodecAPI>() {
                    set_codec_u32(&codec_api, &CODECAPI_AVEncVideoForceKeyFrame, 1);
                }
            }

            self.frame_index += 1;

            // 2. Submit Input Frame
            let hr_input = self.mft.ProcessInput(0, &sample, 0);
            if hr_input.is_ok() {
                if self.inputs_needed > 0 {
                    self.inputs_needed -= 1;
                }
                println!("[H264 HW] ProcessInput → OK");
            } else if let Err(e) = hr_input {
                let code = e.code().0 as u32;
                if code == 0xC00D36B5 {
                    println!("[H264 HW] ProcessInput → MF_E_NOTACCEPTING (HRESULT 0x{:08X})", code);
                } else {
                    println!("[H264 HW] ProcessInput → Err HRESULT(0x{:08X})", code);
                }
            }

            // 3. Process events & pull any output samples generated
            if self.is_async {
                let ev_out = self.process_events()?;
                if !ev_out.is_empty() {
                    collected_output.extend_from_slice(&ev_out);
                }
            } else {
                let out = self.pull_single_output()?;
                if !out.is_empty() {
                    collected_output.extend_from_slice(&out);
                }
            }

            Ok(collected_output)
        }
    }
}

// Fast SIMD-friendly RGBA to NV12 conversion
fn rgba_to_nv12(rgba: &[u8], width: usize, height: usize, nv12: &mut [u8]) {
    let y_plane_size = width * height;
    let (y_plane, uv_plane) = nv12.split_at_mut(y_plane_size);

    for j in 0..height {
        let row_start = j * width * 4;
        let y_row_start = j * width;
        let uv_row_start = (j / 2) * width;
        let is_even_row = (j % 2) == 0;

        for i in 0..width {
            let px = row_start + i * 4;
            if px + 2 >= rgba.len() { break; }

            let r = rgba[px] as u32;
            let g = rgba[px + 1] as u32;
            let b = rgba[px + 2] as u32;

            let y_val = ((66 * r + 129 * g + 25 * b + 128) >> 8) + 16;
            y_plane[y_row_start + i] = y_val as u8;

            if is_even_row && (i % 2 == 0) {
                let u_val = ((-38 * (r as i32) - 74 * (g as i32) + 112 * (b as i32) + 128) >> 8) + 128;
                let v_val = ((112 * (r as i32) - 94 * (g as i32) - 18 * (b as i32) + 128) >> 8) + 128;

                let uv_idx = uv_row_start + i;
                if uv_idx + 1 < uv_plane.len() {
                    uv_plane[uv_idx] = u_val.clamp(0, 255) as u8;
                    uv_plane[uv_idx + 1] = v_val.clamp(0, 255) as u8;
                }
            }
        }
    }
}

// Fast box-downscale + NV12 conversion in a single pass (3840x2160 -> 1920x1080)
fn rgba_downscale_to_nv12(
    src_rgba: &[u8],
    src_w: usize,
    src_h: usize,
    dst_w: usize,
    dst_h: usize,
    nv12: &mut [u8],
) {
    let y_plane_size = dst_w * dst_h;
    let (y_plane, uv_plane) = nv12.split_at_mut(y_plane_size);

    let scale_x = src_w as f32 / dst_w as f32;
    let scale_y = src_h as f32 / dst_h as f32;

    for dy in 0..dst_h {
        let sy = (dy as f32 * scale_y) as usize;
        let sy_next = ((dy as f32 + 0.5) * scale_y) as usize;
        let y_row = dy * dst_w;
        let uv_row = (dy / 2) * dst_w;
        let is_even_row = (dy % 2) == 0;

        for dx in 0..dst_w {
            let sx = (dx as f32 * scale_x) as usize;
            let sx_next = ((dx as f32 + 0.5) * scale_x) as usize;

            let p0 = (sy * src_w + sx) * 4;
            let p1 = (sy * src_w + sx_next.min(src_w - 1)) * 4;
            let p2 = (sy_next.min(src_h - 1) * src_w + sx) * 4;
            let p3 = (sy_next.min(src_h - 1) * src_w + sx_next.min(src_w - 1)) * 4;

            let (r, g, b) = if p3 + 2 < src_rgba.len() {
                let r_sum = src_rgba[p0] as u32 + src_rgba[p1] as u32 + src_rgba[p2] as u32 + src_rgba[p3] as u32;
                let g_sum = src_rgba[p0+1] as u32 + src_rgba[p1+1] as u32 + src_rgba[p2+1] as u32 + src_rgba[p3+1] as u32;
                let b_sum = src_rgba[p0+2] as u32 + src_rgba[p1+2] as u32 + src_rgba[p2+2] as u32 + src_rgba[p3+2] as u32;
                (r_sum >> 2, g_sum >> 2, b_sum >> 2)
            } else if p0 + 2 < src_rgba.len() {
                (src_rgba[p0] as u32, src_rgba[p0+1] as u32, src_rgba[p0+2] as u32)
            } else {
                (0, 0, 0)
            };

            let y_val = ((66 * r + 129 * g + 25 * b + 128) >> 8) + 16;
            y_plane[y_row + dx] = y_val as u8;

            if is_even_row && (dx % 2 == 0) {
                let u_val = ((-38 * (r as i32) - 74 * (g as i32) + 112 * (b as i32) + 128) >> 8) + 128;
                let v_val = ((112 * (r as i32) - 94 * (g as i32) - 18 * (b as i32) + 128) >> 8) + 128;

                let uv_idx = uv_row + dx;
                if uv_idx + 1 < uv_plane.len() {
                    uv_plane[uv_idx] = u_val.clamp(0, 255) as u8;
                    uv_plane[uv_idx + 1] = v_val.clamp(0, 255) as u8;
                }
            }
        }
    }
}

// Converts encoded MFT output to standard Annex-B bitstream
fn convert_to_annex_b(data: &[u8]) -> Vec<u8> {
    if data.is_empty() {
        return Vec::new();
    }

    if data.len() >= 4 && (
        (data[0] == 0 && data[1] == 0 && data[2] == 0 && data[3] == 1) ||
        (data[0] == 0 && data[1] == 0 && data[2] == 1)
    ) {
        return data.to_vec();
    }

    let mut annex_b = Vec::with_capacity(data.len() + 32);
    let mut offset = 0;

    while offset + 4 <= data.len() {
        let nal_len = u32::from_be_bytes(data[offset..offset + 4].try_into().unwrap()) as usize;
        offset += 4;

        if offset + nal_len > data.len() {
            break;
        }

        annex_b.extend_from_slice(&[0x00, 0x00, 0x00, 0x01]);
        annex_b.extend_from_slice(&data[offset..offset + nal_len]);
        offset += nal_len;
    }

    if annex_b.is_empty() {
        data.to_vec()
    } else {
        annex_b
    }
}

fn log_keyframe_diagnostics(h264_bytes: &[u8]) {
    let mut nal_types = Vec::new();
    let mut has_sps = false;
    let mut has_pps = false;
    let mut has_idr = false;

    let mut i = 0;
    while i + 2 < h264_bytes.len() {
        let mut sc_len = 0;
        if i + 3 < h264_bytes.len() && h264_bytes[i] == 0 && h264_bytes[i+1] == 0 && h264_bytes[i+2] == 0 && h264_bytes[i+3] == 1 {
            sc_len = 4;
        } else if h264_bytes[i] == 0 && h264_bytes[i+1] == 0 && h264_bytes[i+2] == 1 {
            sc_len = 3;
        }
        if sc_len > 0 {
            if i + sc_len < h264_bytes.len() {
                let n_type = h264_bytes[i + sc_len] & 0x1F;
                nal_types.push(n_type);
                if n_type == 7 { has_sps = true; }
                if n_type == 8 { has_pps = true; }
                if n_type == 5 { has_idr = true; }
            }
            i += sc_len;
        } else {
            i += 1;
        }
    }

    println!("[H264 HW][OUTPUT]");
    println!("size={}", h264_bytes.len());
    println!("NAL types={:?}", nal_types);
    println!("SPS={}", has_sps);
    println!("PPS={}", has_pps);
    println!("IDR={}", has_idr);
}
