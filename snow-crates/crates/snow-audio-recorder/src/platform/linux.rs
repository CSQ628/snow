//! PipeWire capture for the default sink monitor and the default microphone.

use std::mem;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, SyncSender};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use pipewire as pw;
use pw::properties::properties;
use pw::spa;
use spa::param::format::{MediaSubtype, MediaType};
use spa::param::format_utils;
use spa::pod::Pod;

use crate::backend::{AudioBackend, AudioBackendKind, AudioRecorderEngine, EngineEvent};
use crate::device::{AudioDeviceInfo, DeviceFlow};
use crate::error::{AudioError, AudioResult};
use crate::format::AudioFormat;
use crate::packet::{AudioEvent, AudioPacket, AudioPacketMetadata, AudioSourceKind};
use crate::session::{AudioStreamConfig, SourceConfig};
use crate::AudioSourceStatus;

const MONITOR_ID: &str = "pipewire:default-monitor";
const MICROPHONE_ID: &str = "pipewire:default-source";

pub(crate) struct PipewireBackend;

impl AudioBackend for PipewireBackend {
    fn enumerate_devices(&self, flow: DeviceFlow) -> AudioResult<Vec<AudioDeviceInfo>> {
        let device = match flow {
            DeviceFlow::Render => AudioDeviceInfo {
                id: MONITOR_ID.into(),
                name: "System audio".into(),
                is_default: true,
                is_active: true,
                flow,
            },
            DeviceFlow::Capture => AudioDeviceInfo {
                id: MICROPHONE_ID.into(),
                name: "Microphone".into(),
                is_default: true,
                is_active: true,
                flow,
            },
        };
        Ok(vec![device])
    }

    fn create_engine(
        &self,
        config: AudioStreamConfig,
    ) -> AudioResult<Box<dyn AudioRecorderEngine>> {
        PipewireEngine::start(config).map(|engine| Box::new(engine) as _)
    }
}

pub(crate) fn build(kind: AudioBackendKind) -> AudioResult<PipewireBackend> {
    if !matches!(kind, AudioBackendKind::Auto) {
        return Err(AudioError::BackendUnavailable(
            "WASAPI and Core Audio require their own platforms".into(),
        ));
    }
    Ok(PipewireBackend)
}

struct PipewireEngine {
    packets: Receiver<AudioPacket>,
    statuses: Arc<Mutex<[AudioSourceStatus; 2]>>,
    loop_ptr: usize,
    worker: Option<JoinHandle<()>>,
}

impl PipewireEngine {
    fn start(config: AudioStreamConfig) -> AudioResult<Self> {
        let (boot_tx, boot_rx) = mpsc::channel();
        let (packet_tx, packet_rx) = mpsc::sync_channel(64);
        let statuses = Arc::new(Mutex::new([
            source_status(&config.system),
            source_status(&config.microphone),
        ]));
        let worker_status = Arc::clone(&statuses);
        let worker = std::thread::Builder::new()
            .name("snow-pipewire-audio".into())
            .spawn(move || worker(config, packet_tx, worker_status, boot_tx))
            .map_err(|error| AudioError::platform(anyhow::anyhow!(error)))?;
        let loop_ptr = match boot_rx.recv() {
            Ok(Ok(ptr)) => ptr,
            Ok(Err(error)) => {
                let _ = worker.join();
                return Err(error);
            }
            Err(_) => {
                let _ = worker.join();
                return Err(AudioError::BackendUnavailable(
                    "PipeWire audio thread exited before the stream started".into(),
                ));
            }
        };
        Ok(Self {
            packets: packet_rx,
            statuses,
            loop_ptr,
            worker: Some(worker),
        })
    }
}

impl Drop for PipewireEngine {
    fn drop(&mut self) {
        if self.loop_ptr != 0 {
            unsafe { pw::sys::pw_main_loop_quit(self.loop_ptr as *mut _) };
        }
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

impl AudioRecorderEngine for PipewireEngine {
    fn poll(&mut self, timeout: Duration) -> AudioResult<EngineEvent> {
        let first = match self.packets.recv_timeout(timeout) {
            Ok(packet) => packet,
            Err(RecvTimeoutError::Timeout) => return Ok(EngineEvent::Idle),
            Err(RecvTimeoutError::Disconnected) => {
                return Err(AudioError::BackendUnavailable(
                    "PipeWire audio stream ended".into(),
                ));
            }
        };
        let mut events = vec![AudioEvent::Packet(first)];
        while let Ok(packet) = self.packets.try_recv() {
            events.push(AudioEvent::Packet(packet));
        }
        Ok(EngineEvent::Events(events))
    }

    fn source_states(&self) -> Option<[AudioSourceStatus; 2]> {
        self.statuses.lock().ok().map(|guard| *guard)
    }
}

fn source_status(config: &SourceConfig) -> AudioSourceStatus {
    if config.enabled {
        AudioSourceStatus::Starting
    } else {
        AudioSourceStatus::Disabled
    }
}

struct CaptureUser {
    format: spa::param::audio::AudioInfoRaw,
    sender: SyncSender<AudioPacket>,
    source: AudioSourceKind,
    output: AudioFormat,
    sequence: u64,
    statuses: Arc<Mutex<[AudioSourceStatus; 2]>>,
}

fn worker(
    config: AudioStreamConfig,
    sender: SyncSender<AudioPacket>,
    statuses: Arc<Mutex<[AudioSourceStatus; 2]>>,
    boot: mpsc::Sender<AudioResult<usize>>,
) {
    let started = (|| {
        pw::init();
        let mainloop = pw::main_loop::MainLoop::new(None).map_err(|error| {
            AudioError::BackendUnavailable(format!("PipeWire main loop: {error}"))
        })?;
        let context = pw::context::Context::new(&mainloop).map_err(|error| {
            AudioError::BackendUnavailable(format!("PipeWire context: {error}"))
        })?;
        let core = context.connect(None).map_err(|error| {
            AudioError::BackendUnavailable(format!("PipeWire connection: {error}"))
        })?;
        let mut listeners = Vec::new();
        if config.system.enabled {
            listeners.push(open_stream(
                &core,
                "snow-shot-system",
                true,
                AudioSourceKind::System,
                &config.system,
                sender.clone(),
                Arc::clone(&statuses),
            )?);
        }
        if config.microphone.enabled {
            listeners.push(open_stream(
                &core,
                "snow-shot-microphone",
                false,
                AudioSourceKind::Microphone,
                &config.microphone,
                sender,
                Arc::clone(&statuses),
            )?);
        }
        Ok((mainloop, listeners))
    })();
    let (mainloop, _listeners) = match started {
        Ok(value) => value,
        Err(error) => {
            let _ = boot.send(Err(error));
            return;
        }
    };
    let _ = boot.send(Ok(mainloop.as_raw_ptr() as usize));
    mainloop.run();
}

fn open_stream(
    core: &pw::core::Core,
    name: &str,
    monitor: bool,
    source: AudioSourceKind,
    config: &SourceConfig,
    sender: SyncSender<AudioPacket>,
    statuses: Arc<Mutex<[AudioSourceStatus; 2]>>,
) -> AudioResult<pw::stream::StreamListener<CaptureUser>> {
    let mut props = properties! {
        *pw::keys::MEDIA_TYPE => "Audio",
        *pw::keys::MEDIA_CATEGORY => "Capture",
        *pw::keys::MEDIA_ROLE => "Music",
    };
    if monitor {
        props.insert("stream.capture.sink", "true");
    }
    let stream = pw::stream::Stream::new(core, name, props)
        .map_err(|error| AudioError::BackendUnavailable(format!("PipeWire stream: {error}")))?;
    let user = CaptureUser {
        format: spa::param::audio::AudioInfoRaw::default(),
        sender,
        source,
        output: config.output_format,
        sequence: 0,
        statuses,
    };
    let listener = stream
        .add_local_listener_with_user_data(user)
        .param_changed(|_, user, id, param| {
            let Some(param) = param else {
                return;
            };
            if id != pw::spa::param::ParamType::Format.as_raw() {
                return;
            }
            let Ok((media_type, media_subtype)) = format_utils::parse_format(param) else {
                return;
            };
            if media_type != MediaType::Audio || media_subtype != MediaSubtype::Raw {
                return;
            }
            if user.format.parse(param).is_ok() {
                let index = source_index(user.source);
                if let Ok(mut statuses) = user.statuses.lock() {
                    statuses[index] = AudioSourceStatus::Ready;
                }
            }
        })
        .process(|stream, user| {
            let Some(mut buffer) = stream.dequeue_buffer() else {
                return;
            };
            let datas = buffer.datas_mut();
            if datas.is_empty() {
                return;
            }
            let data = &mut datas[0];
            let channels = user.format.channels().max(1) as usize;
            let sample_format = user.format.format();
            let sample_bytes = if sample_format == spa::param::audio::AudioFormat::S16LE {
                2
            } else {
                mem::size_of::<f32>()
            };
            if sample_bytes == 0 || channels == 0 {
                return;
            }
            let samples = data.chunk().size() as usize / sample_bytes;
            let Some(bytes) = data.data() else {
                return;
            };
            let frames = samples / channels;
            if frames == 0 {
                return;
            }
            let native_rate = user.format.rate().max(1);
            let pcm = to_i16(bytes, samples, sample_format);
            let mixed = mix_channels(&pcm, channels, user.output.channels as usize);
            let resampled = resample(
                &mixed,
                user.output.channels as usize,
                native_rate,
                user.output.sample_rate.max(1),
            );
            let out_channels = user.output.channels as usize;
            if out_channels == 0 || resampled.len() < out_channels {
                return;
            }
            user.sequence = user.sequence.wrapping_add(1);
            let packet = AudioPacket {
                source: user.source,
                format: user.output,
                frames: (resampled.len() / out_channels) as u32,
                data: resampled,
                metadata: AudioPacketMetadata {
                    sequence: user.sequence,
                    stream_timestamp: Some(snow_core::timestamp::StreamTimestamp {
                        instant: Instant::now(),
                        raw_os_ticks: None,
                        tick_format: snow_core::timestamp::TickFormat::Hns100,
                    }),
                    ..AudioPacketMetadata::default()
                },
            };
            let _ = user.sender.try_send(packet);
        })
        .register()
        .map_err(|error| AudioError::platform(anyhow::anyhow!(error)))?;
    let mut audio = spa::param::audio::AudioInfoRaw::new();
    audio.set_format(spa::param::audio::AudioFormat::F32LE);
    audio.set_rate(config.output_format.sample_rate);
    audio.set_channels(config.output_format.channels as u32);
    let object = pw::spa::pod::Object {
        type_: pw::spa::utils::SpaTypes::ObjectParamFormat.as_raw(),
        id: pw::spa::param::ParamType::EnumFormat.as_raw(),
        properties: audio.into(),
    };
    let values = pw::spa::pod::serialize::PodSerializer::serialize(
        std::io::Cursor::new(Vec::new()),
        &pw::spa::pod::Value::Object(object),
    )
    .map_err(|error| AudioError::platform(anyhow::anyhow!("{error:?}")))?
    .0
    .into_inner();
    let mut params = [Pod::from_bytes(&values).ok_or_else(|| {
        AudioError::BackendUnavailable("PipeWire rejected the audio format".into())
    })?];
    stream
        .connect(
            spa::utils::Direction::Input,
            None,
            pw::stream::StreamFlags::AUTOCONNECT
                | pw::stream::StreamFlags::MAP_BUFFERS
                | pw::stream::StreamFlags::RT_PROCESS,
            &mut params,
        )
        .map_err(|error| AudioError::BackendUnavailable(format!("PipeWire connect: {error}")))?;
    Ok(listener)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pipewire_lists_default_system_and_microphone() {
        let backend = build(AudioBackendKind::Auto).expect("pipewire backend");
        let outputs = backend
            .enumerate_devices(DeviceFlow::Render)
            .expect("outputs");
        let inputs = backend
            .enumerate_devices(DeviceFlow::Capture)
            .expect("inputs");
        assert!(outputs.iter().any(|device| device.is_default && device.id == MONITOR_ID));
        assert!(inputs.iter().any(|device| device.is_default && device.id == MICROPHONE_ID));
    }

    #[test]
    fn pipewire_engine_polls_when_the_socket_exists() {
        let runtime = std::env::var("XDG_RUNTIME_DIR").unwrap_or_default();
        let socket = std::path::PathBuf::from(runtime).join("pipewire-0");
        if !socket.exists() {
            return;
        }
        let mut config = AudioStreamConfig::default();
        config.microphone.enabled = false;
        config.system.required = false;
        let backend = build(AudioBackendKind::Auto).expect("pipewire backend");
        let mut engine = backend.create_engine(config).expect("engine");
        let event = engine
            .poll(Duration::from_millis(400))
            .expect("poll");
        assert!(matches!(event, EngineEvent::Idle | EngineEvent::Events(_)));
    }
}

fn source_index(source: AudioSourceKind) -> usize {
    match source {
        AudioSourceKind::System => 0,
        AudioSourceKind::Microphone => 1,
    }
}

fn to_i16(bytes: &[u8], samples: usize, format: spa::param::audio::AudioFormat) -> Vec<i16> {
    if format == spa::param::audio::AudioFormat::S16LE {
        bytes
            .chunks_exact(2)
            .take(samples)
            .map(|chunk| i16::from_le_bytes([chunk[0], chunk[1]]))
            .collect()
    } else {
        bytes
            .chunks_exact(4)
            .take(samples)
            .map(|chunk| {
                let sample = f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
                (sample.clamp(-1.0, 1.0) * i16::MAX as f32) as i16
            })
            .collect()
    }
}

fn mix_channels(samples: &[i16], native: usize, output: usize) -> Vec<i16> {
    if native == 0 || output == 0 {
        return Vec::new();
    }
    if native == output {
        return samples.to_vec();
    }
    let frames = samples.len() / native;
    let mut mixed = Vec::with_capacity(frames * output);
    for frame in 0..frames {
        let start = frame * native;
        if output == 1 {
            let sum: i32 = samples[start..start + native].iter().map(|s| *s as i32).sum();
            mixed.push((sum / native as i32) as i16);
        } else {
            for channel in 0..output {
                let source = if channel < native { channel } else { native - 1 };
                mixed.push(samples[start + source]);
            }
        }
    }
    mixed
}

fn resample(samples: &[i16], channels: usize, from_rate: u32, to_rate: u32) -> Vec<i16> {
    if channels == 0 || from_rate == 0 || to_rate == 0 || from_rate == to_rate {
        return samples.to_vec();
    }
    let frames = samples.len() / channels;
    if frames == 0 {
        return Vec::new();
    }
    let out_frames = ((frames as u64 * u64::from(to_rate)) / u64::from(from_rate)).max(1) as usize;
    let mut output = Vec::with_capacity(out_frames * channels);
    for frame in 0..out_frames {
        let position = frame as f64 * f64::from(from_rate) / f64::from(to_rate);
        let left = position.floor() as usize;
        let right = (left + 1).min(frames - 1);
        let fraction = position - left as f64;
        for channel in 0..channels {
            let a = samples[left * channels + channel] as f64;
            let b = samples[right * channels + channel] as f64;
            output.push((a + (b - a) * fraction) as i16);
        }
    }
    output
}
