//! V4L2 capture. Frames are converted from YUYV to opaque RGBA.

use std::os::fd::RawFd;
use std::time::{Duration, Instant};

use crate::{CameraBackend, CaptureOptions, RawFrame};

const VIDEO_CAPTURE: u32 = 1;
const MEMORY_MMAP: u32 = 1;
const YUYV: u32 = 1448695129;
const VIDIOC_QUERYCAP: libc::Ioctl = 2154321408;
const VIDIOC_S_FMT: libc::Ioctl = 3234878981;
const VIDIOC_REQBUFS: libc::Ioctl = 3222558216;
const VIDIOC_QUERYBUF: libc::Ioctl = 3227014665;
const VIDIOC_QBUF: libc::Ioctl = 3227014671;
const VIDIOC_DQBUF: libc::Ioctl = 3227014673;
const VIDIOC_STREAMON: libc::Ioctl = 1074026002;
const VIDIOC_STREAMOFF: libc::Ioctl = 1074026003;

struct Mapping {
    ptr: *mut u8,
    len: usize,
}

struct Camera {
    fd: RawFd,
    width: u32,
    height: u32,
    stride: u32,
    maps: Vec<Mapping>,
}

pub fn open(options: CaptureOptions) -> Result<Box<dyn CameraBackend>, String> {
    for index in 0..8 {
        if let Ok(camera) = Camera::open(index, options) {
            return Ok(Box::new(camera));
        }
    }
    Err("no V4L2 camera is available".into())
}

impl Camera {
    fn open(index: u32, options: CaptureOptions) -> Result<Self, String> {
        let path = format!("/dev/video{index}\0");
        let fd = unsafe { libc::open(path.as_ptr().cast(), libc::O_RDWR | libc::O_NONBLOCK) };
        if fd < 0 {
            return Err("camera device did not open".into());
        }
        let mut capability = [0u8; 104];
        if unsafe { libc::ioctl(fd, VIDIOC_QUERYCAP, capability.as_mut_ptr()) } < 0 {
            unsafe { libc::close(fd) };
            return Err("camera capability query failed".into());
        }
        let capabilities = u32::from_le_bytes(capability[84..88].try_into().unwrap());
        let device_caps = u32::from_le_bytes(capability[88..92].try_into().unwrap());
        let capture = if device_caps == 0 {
            capabilities
        } else {
            device_caps
        };
        if capture & 1 == 0 {
            unsafe { libc::close(fd) };
            return Err("device is not a capture camera".into());
        }
        let mut format = [0u8; 208];
        format[..4].copy_from_slice(&VIDEO_CAPTURE.to_le_bytes());
        let width = options.size.max(1).min(1920);
        let height = width;
        format[8..12].copy_from_slice(&width.to_le_bytes());
        format[12..16].copy_from_slice(&height.to_le_bytes());
        format[16..20].copy_from_slice(&YUYV.to_le_bytes());
        if unsafe { libc::ioctl(fd, VIDIOC_S_FMT, format.as_mut_ptr()) } < 0 {
            unsafe { libc::close(fd) };
            return Err("camera rejected YUYV".into());
        }
        let width = u32::from_le_bytes(format[8..12].try_into().unwrap());
        let height = u32::from_le_bytes(format[12..16].try_into().unwrap());
        let stride = u32::from_le_bytes(format[24..28].try_into().unwrap()).max(width * 2);
        let fourcc = u32::from_le_bytes(format[16..20].try_into().unwrap());
        if width == 0 || height == 0 || fourcc != YUYV {
            unsafe { libc::close(fd) };
            return Err("camera did not accept YUYV".into());
        }
        let mut request = [0u8; 20];
        request[..4].copy_from_slice(&4u32.to_le_bytes());
        request[4..8].copy_from_slice(&VIDEO_CAPTURE.to_le_bytes());
        request[8..12].copy_from_slice(&MEMORY_MMAP.to_le_bytes());
        if unsafe { libc::ioctl(fd, VIDIOC_REQBUFS, request.as_mut_ptr()) } < 0 {
            unsafe { libc::close(fd) };
            return Err("camera did not allocate buffers".into());
        }
        let count = u32::from_le_bytes(request[..4].try_into().unwrap()).clamp(1, 4);
        let mut maps = Vec::new();
        for index in 0..count {
            let mut buffer = [0u8; 88];
            buffer[..4].copy_from_slice(&index.to_le_bytes());
            buffer[4..8].copy_from_slice(&VIDEO_CAPTURE.to_le_bytes());
            buffer[60..64].copy_from_slice(&MEMORY_MMAP.to_le_bytes());
            if unsafe { libc::ioctl(fd, VIDIOC_QUERYBUF, buffer.as_mut_ptr()) } < 0 {
                continue;
            }
            let length = u32::from_le_bytes(buffer[72..76].try_into().unwrap()) as usize;
            let offset = u32::from_le_bytes(buffer[64..68].try_into().unwrap()) as libc::off_t;
            let ptr = unsafe {
                libc::mmap(
                    std::ptr::null_mut(),
                    length,
                    libc::PROT_READ | libc::PROT_WRITE,
                    libc::MAP_SHARED,
                    fd,
                    offset,
                )
            };
            if ptr == libc::MAP_FAILED {
                continue;
            }
            if unsafe { libc::ioctl(fd, VIDIOC_QBUF, buffer.as_mut_ptr()) } < 0 {
                unsafe { libc::munmap(ptr, length) };
                continue;
            }
            maps.push(Mapping {
                ptr: ptr.cast(),
                len: length,
            });
        }
        let mut kind = VIDEO_CAPTURE;
        if maps.is_empty() || unsafe { libc::ioctl(fd, VIDIOC_STREAMON, &mut kind) } < 0 {
            unsafe { libc::close(fd) };
            return Err("camera stream did not start".into());
        }
        Ok(Self {
            fd,
            width,
            height,
            stride,
            maps,
        })
    }
}

impl Drop for Camera {
    fn drop(&mut self) {
        let mut kind = VIDEO_CAPTURE;
        unsafe { libc::ioctl(self.fd, VIDIOC_STREAMOFF, &mut kind) };
        for map in &self.maps {
            unsafe { libc::munmap(map.ptr.cast(), map.len) };
        }
        unsafe { libc::close(self.fd) };
    }
}

impl CameraBackend for Camera {
    fn poll(&mut self, timeout: Duration) -> Result<Option<RawFrame>, String> {
        let mut poll = libc::pollfd {
            fd: self.fd,
            events: libc::POLLIN,
            revents: 0,
        };
        let millis = timeout.as_millis().min(i32::MAX as u128) as i32;
        let ready = unsafe { libc::poll(&mut poll, 1, millis) };
        if ready < 0 {
            return Err("camera poll failed".into());
        }
        if ready == 0 {
            return Ok(None);
        }
        let mut buffer = [0u8; 88];
        buffer[4..8].copy_from_slice(&VIDEO_CAPTURE.to_le_bytes());
        buffer[60..64].copy_from_slice(&MEMORY_MMAP.to_le_bytes());
        if unsafe { libc::ioctl(self.fd, VIDIOC_DQBUF, buffer.as_mut_ptr()) } < 0 {
            return Err("camera frame dequeue failed".into());
        }
        let index = u32::from_le_bytes(buffer[..4].try_into().unwrap()) as usize;
        let used = u32::from_le_bytes(buffer[8..12].try_into().unwrap()) as usize;
        let rgba = self.maps.get(index).map(|map| {
            let len = used.min(map.len);
            let bytes = unsafe { std::slice::from_raw_parts(map.ptr, len) };
            yuyv_to_rgba(bytes, self.width, self.height, self.stride as usize)
        });
        unsafe { libc::ioctl(self.fd, VIDIOC_QBUF, buffer.as_mut_ptr()) };
        Ok(rgba.map(|pixels| RawFrame {
            width: self.width,
            height: self.height,
            stride: self.width as usize * 4,
            rgba: pixels,
            at: Instant::now(),
        }))
    }
}

fn yuyv_to_rgba(src: &[u8], width: u32, height: u32, stride: usize) -> Vec<u8> {
    let width = width as usize;
    let height = height as usize;
    let mut rgba = vec![255u8; width * height * 4];
    for y in 0..height {
        let row = y * stride;
        if row + width * 2 > src.len() {
            break;
        }
        for x in (0..width).step_by(2) {
            let offset = row + x * 2;
            if offset + 3 >= src.len() {
                break;
            }
            let y0 = src[offset] as i32;
            let u = src[offset + 1] as i32 - 128;
            let y1 = src[offset + 2] as i32;
            let v = src[offset + 3] as i32 - 128;
            write_rgb(&mut rgba, y * width + x, y0, u, v);
            if x + 1 < width {
                write_rgb(&mut rgba, y * width + x + 1, y1, u, v);
            }
        }
    }
    rgba
}

fn write_rgb(rgba: &mut [u8], pixel: usize, y: i32, u: i32, v: i32) {
    let c = y - 16;
    let r = (298 * c + 409 * v + 128) >> 8;
    let g = (298 * c - 100 * u - 208 * v + 128) >> 8;
    let b = (298 * c + 516 * u + 128) >> 8;
    let dest = pixel * 4;
    rgba[dest] = r.clamp(0, 255) as u8;
    rgba[dest + 1] = g.clamp(0, 255) as u8;
    rgba[dest + 2] = b.clamp(0, 255) as u8;
}

unsafe impl Send for Camera {}
