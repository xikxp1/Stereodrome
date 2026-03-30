use rodio::DeviceSinkBuilder;
use rodio::cpal::traits::{DeviceTrait, HostTrait};
use rodio::cpal::{self, Device};
use rodio::stream::MixerDeviceSink;

use crate::error::{AppError, AppResult};

#[derive(Debug, Clone, serde::Serialize)]
pub struct AudioOutputDevice {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Clone, Default)]
pub struct AudioOutputRouteState {
    pub active_device_id: Option<String>,
    pub active_device_name: Option<String>,
    pub using_default_fallback: bool,
    #[cfg(target_os = "macos")]
    pub system_default_bound: bool,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct AudioOutputState {
    pub devices: Vec<AudioOutputDevice>,
    pub active_device_id: Option<String>,
    pub active_device_name: Option<String>,
    pub using_default_fallback: bool,
}

struct DeviceEntry {
    device: Device,
    info: AudioOutputDevice,
}

fn device_name(device: &Device) -> String {
    device
        .description()
        .ok()
        .map(|description| description.name().to_string())
        .filter(|name| !name.trim().is_empty())
        .unwrap_or_else(|| "Unknown output device".to_string())
}

fn device_id(device: &Device) -> Option<String> {
    device.id().ok().map(|id| id.to_string())
}

fn device_info(device: &Device) -> Option<AudioOutputDevice> {
    Some(AudioOutputDevice {
        id: device_id(device)?,
        name: device_name(device),
    })
}

fn output_devices_with_info() -> AppResult<Vec<DeviceEntry>> {
    let devices = cpal::default_host()
        .output_devices()
        .map_err(|e| AppError::Audio(format!("Failed to enumerate output devices: {e}")))?;

    Ok(devices
        .filter_map(|device| {
            Some(DeviceEntry {
                info: device_info(&device)?,
                device,
            })
        })
        .collect())
}

fn default_output_device_with_info() -> Option<(Device, AudioOutputDevice)> {
    let device = cpal::default_host().default_output_device()?;
    let info = device_info(&device)?;
    Some((device, info))
}

#[cfg(target_os = "macos")]
pub fn current_default_output_device_id() -> Option<String> {
    default_output_device_with_info().map(|(_, info)| info.id)
}

pub fn list_audio_output_devices() -> AppResult<Vec<AudioOutputDevice>> {
    Ok(output_devices_with_info()?
        .into_iter()
        .map(|entry| entry.info)
        .collect())
}

fn no_output_device_error() -> AppError {
    AppError::Audio("No audio output device is available".to_string())
}

fn open_device_sink<E>(device: Device, error_callback: E) -> AppResult<MixerDeviceSink>
where
    E: FnMut(cpal::StreamError) + Send + Clone + 'static,
{
    DeviceSinkBuilder::from_device(device)
        .map_err(|e| AppError::Audio(format!("Failed to configure audio output device: {e}")))?
        .with_error_callback(error_callback)
        .open_sink_or_fallback()
        .map_err(|e| AppError::Audio(format!("Failed to open audio output stream: {e}")))
}

pub fn open_output_stream<E>(
    preferred_device_id: Option<&str>,
    error_callback: E,
) -> AppResult<(MixerDeviceSink, AudioOutputRouteState)>
where
    E: FnMut(cpal::StreamError) + Send + Clone + 'static,
{
    let devices = output_devices_with_info()?;
    let default_device = default_output_device_with_info();

    match resolve_device_choice(
        preferred_device_id,
        &devices
            .iter()
            .map(|entry| entry.info.id.clone())
            .collect::<Vec<_>>(),
        default_device.as_ref().map(|(_, info)| info.id.as_str()),
    )? {
        ResolvedDeviceChoice::Preferred(preferred_id) => {
            let entry = devices
                .into_iter()
                .find(|entry| entry.info.id == preferred_id)
                .ok_or_else(no_output_device_error)?;

            match open_device_sink(entry.device, error_callback.clone()) {
                Ok(stream) => Ok((
                    stream,
                    AudioOutputRouteState {
                        active_device_id: Some(entry.info.id),
                        active_device_name: Some(entry.info.name),
                        using_default_fallback: false,
                        #[cfg(target_os = "macos")]
                        system_default_bound: default_device
                            .as_ref()
                            .is_some_and(|(_, info)| info.id == preferred_id),
                    },
                )),
                Err(e) => {
                    let Some((default_device, default_info)) = default_device.clone() else {
                        return Err(e);
                    };

                    if default_info.id == preferred_id {
                        return Err(e);
                    }

                    open_device_sink(default_device, error_callback).map(|stream| {
                        (
                            stream,
                            AudioOutputRouteState {
                                active_device_id: Some(default_info.id),
                                active_device_name: Some(default_info.name),
                                using_default_fallback: true,
                                #[cfg(target_os = "macos")]
                                system_default_bound: false,
                            },
                        )
                    })
                }
            }
        }
        ResolvedDeviceChoice::Default => {
            let (device, info) = default_device.ok_or_else(no_output_device_error)?;
            let stream = open_device_sink(device, error_callback)?;
            Ok((
                stream,
                AudioOutputRouteState {
                    active_device_id: Some(info.id),
                    active_device_name: Some(info.name),
                    using_default_fallback: false,
                    #[cfg(target_os = "macos")]
                    system_default_bound: true,
                },
            ))
        }
        ResolvedDeviceChoice::FallbackToDefault => {
            let (device, info) = default_device.ok_or_else(no_output_device_error)?;
            let stream = open_device_sink(device, error_callback)?;
            Ok((
                stream,
                AudioOutputRouteState {
                    active_device_id: Some(info.id),
                    active_device_name: Some(info.name),
                    using_default_fallback: true,
                    #[cfg(target_os = "macos")]
                    system_default_bound: false,
                },
            ))
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ResolvedDeviceChoice {
    Preferred(String),
    Default,
    FallbackToDefault,
}

fn resolve_device_choice(
    preferred_device_id: Option<&str>,
    device_ids: &[String],
    default_device_id: Option<&str>,
) -> AppResult<ResolvedDeviceChoice> {
    match preferred_device_id {
        Some(preferred_id) if device_ids.iter().any(|id| id == preferred_id) => {
            Ok(ResolvedDeviceChoice::Preferred(preferred_id.to_string()))
        }
        Some(_) if default_device_id.is_some() => Ok(ResolvedDeviceChoice::FallbackToDefault),
        Some(_) => Err(no_output_device_error()),
        None if default_device_id.is_some() => Ok(ResolvedDeviceChoice::Default),
        None => Err(no_output_device_error()),
    }
}

#[cfg(test)]
mod tests {
    use super::{ResolvedDeviceChoice, resolve_device_choice};

    #[test]
    fn resolves_preferred_device_when_present() {
        let device_ids = vec!["speaker-a".to_string(), "speaker-b".to_string()];
        let choice =
            resolve_device_choice(Some("speaker-b"), &device_ids, Some("speaker-a")).unwrap();

        assert_eq!(
            choice,
            ResolvedDeviceChoice::Preferred("speaker-b".to_string())
        );
    }

    #[test]
    fn falls_back_to_default_when_preferred_device_is_missing() {
        let device_ids = vec!["speaker-a".to_string()];
        let choice =
            resolve_device_choice(Some("speaker-b"), &device_ids, Some("speaker-a")).unwrap();

        assert_eq!(choice, ResolvedDeviceChoice::FallbackToDefault);
    }

    #[test]
    fn errors_when_no_output_device_is_available() {
        let device_ids = Vec::<String>::new();
        let err = resolve_device_choice(None, &device_ids, None).unwrap_err();

        assert!(err.to_string().contains("No audio output device"));
    }
}
