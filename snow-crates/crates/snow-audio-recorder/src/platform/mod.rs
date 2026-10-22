use std::sync::Arc;

use crate::backend::{AudioBackend, AudioBackendKind};
#[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
use crate::device::AudioDeviceInfo;
#[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
use crate::error::AudioError;
use crate::error::AudioResult;
#[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
use crate::session::AudioStreamConfig;

#[cfg(target_os = "windows")]
pub(crate) mod windows;

#[cfg(target_os = "linux")]
mod linux;

#[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
struct UnsupportedBackend;

#[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
impl AudioBackend for UnsupportedBackend {
    fn enumerate_devices(
        &self,
        _flow: crate::device::DeviceFlow,
    ) -> AudioResult<Vec<AudioDeviceInfo>> {
        Err(AudioError::platform(anyhow::anyhow!(
            "audio capture is only supported on Windows"
        )))
    }

    fn create_engine(
        &self,
        _config: AudioStreamConfig,
    ) -> AudioResult<Box<dyn crate::backend::AudioRecorderEngine>> {
        Err(AudioError::platform(anyhow::anyhow!(
            "audio capture is only supported on Windows"
        )))
    }
}

#[cfg(target_os = "windows")]
pub(crate) fn build_backend(kind: AudioBackendKind) -> AudioResult<Arc<dyn AudioBackend>> {
    Ok(Arc::new(windows::WasapiBackend::new(kind)?))
}

#[cfg(target_os = "linux")]
pub(crate) fn build_backend(kind: AudioBackendKind) -> AudioResult<Arc<dyn AudioBackend>> {
    Ok(Arc::new(linux::build(kind)?))
}

#[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
pub(crate) fn build_backend(_kind: AudioBackendKind) -> AudioResult<Arc<dyn AudioBackend>> {
    Ok(Arc::new(UnsupportedBackend))
}

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
pub(crate) fn build_backend(kind: AudioBackendKind) -> AudioResult<Arc<dyn AudioBackend>> {
    if !matches!(kind, AudioBackendKind::Auto | AudioBackendKind::CoreAudio) {
        return Err(crate::error::AudioError::BackendUnavailable(
            "WASAPI requires Windows".into(),
        ));
    }
    Ok(Arc::new(macos::MacAudioBackend))
}
